/**
 * 双形态 `invokeCmd` 的分发层：Tauri shell 走 `invoke`，Web 形态走
 * `serve-web` 的 `/api/*`。命令名表在 `./commands`，REST 路由表在 `./web-routes`。
 *
 * 两种分发形态：
 *   - Tauri shell → 存在 `__TAURI_INTERNALS__`，走 `invoke`。
 *   - Web / Dev    → 落到 `webInvokeCmd`，打 axum `serve-web` 宿主机的 `/api/*`。
 *
 * 在模块加载期判定形态是刻意的：会话内切换形态需要重新加载（页面加载时运行时即固定）。
 */
import { invoke as tauriInvoke } from '@tauri-apps/api/core';

import { COMMANDS, type TauriCommand } from './tauri-commands/commands';
import { readBearerFromStorage } from './tauri-commands/bearer';
import { WEB_ROUTES, type RouteSpec } from './tauri-commands/web-routes';

export { COMMANDS, type TauriCommand } from './tauri-commands/commands';
export {
  WEB_BEARER_STORAGE_KEY,
  writeBearerToStorage,
  clearBearerFromStorage,
  readBearerFromStorage,
  ingestBearerFromUrl,
} from './tauri-commands/bearer';

export const IS_TAURI_SHELL: boolean =
  typeof window !== 'undefined' &&
  Boolean(
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ ||
    (window as { __TAURI__?: unknown }).__TAURI__,
  );

/**
 * `invokeCmd` 的逐调用选项。目前只有中止信号；做成对象以便将来加标志
 * （超时、重试策略等）而不破坏调用点。
 */
export interface CallApiOptions {
  /** 中止在途的 IPC 调用。Tauri 形态：promise reject 但 Rust 继续跑；Web 形态：原生 fetch abort。 */
  signal?: AbortSignal;
}

export function invokeCmd<T>(
  cmd: TauriCommand,
  args?: Record<string, unknown>,
  options?: CallApiOptions,
): Promise<T> {
  if (IS_TAURI_SHELL) {
    // Tauri 2 的 `invoke` 不接受 AbortSignal —— 在这里丢掉，调用点就不必按形态分支。
    // 需要在 Tauri 下中止的调用方应自己包装 promise（或传 `options.signal` 并监听 `abort`）。
    return tauriInvoke<T>(cmd, args ?? {});
  }
  return webInvokeCmd<T>(cmd, args ?? {}, options);
}

async function webInvokeCmd<T>(
  cmd: TauriCommand,
  args: Record<string, unknown>,
  options?: CallApiOptions,
): Promise<T> {
  const spec = WEB_ROUTES[cmd];
  if (!spec) {
    throw new Error(
      `Command "${cmd}" is not exposed in the web shape. Add a REST entry in src/lib/tauri-commands.ts + src-tauri/src/web_api/ to enable it.`,
    );
  }
  // `mcpAutoStartGet` 保持 GET 后挑一行的形状，因为该端点仍返回整张 preferences
  // map 而渲染层只要 `mcp.auto_start` 那一行。`requestSettingsGet` 现在有专用端点
  // （`/api/request-settings`），无需特殊处理。
  if (cmd === COMMANDS.mcpAutoStartGet) {
    const map = await webFetch<Record<string, unknown>>('GET', spec.path(args), null, options);
    return (map['mcp.auto_start'] === true) as unknown as T;
  }

  const body = spec.body ? spec.body(args) : args;
  return webFetch<T>(spec.method, spec.path(args), body, options);
}

/**
 * 本部署所挂载的反向代理前缀，或 `''`。
 *
 * `serve-web` 把整个路由 `nest` 在 `--public-base-path`
 * （`SIMPLE_POST_PUBLIC_BASE_PATH`）下，并在模块入口运行前把该值注入 SPA shell
 * 的 `window.__SIMPLE_POST_BASE__`。必须在**运行时**注入的原因：容器里前缀是环境
 * 变量，一个镜像要同时服务 `https://host/` 与 `https://host/sp/`。
 *
 * `import.meta.env.BASE_URL` 不是替代品 —— 它是 `vite build --base` 在构建期烤
 * 进去的常量，而镜像从不传这个参数。读它会在前缀下报告 `''` 并把每个请求静默发到
 * 裸根路径，而嵌套路由从不提供裸根。
 *
 * 与 `crates/simple-post-server/src/web_api/static_files.rs` 里的 `BASE_GLOBAL`
 * 保持一致。根部署不注入任何东西，故 `''` 这个默认值正是保住根绝对行为的东西。
 */
export function readApiBase(): string {
  // 与 `readBearerFromStorage` 同样的守卫：这些模块级 helper 在单元测试里跑在
  // Node 下，那里没有 `window`。不守卫会让每个切片测试都变成 ReferenceError。
  if (typeof window === 'undefined') return '';
  const injected = (window as unknown as Record<string, unknown>).__SIMPLE_POST_BASE__;
  return typeof injected === 'string' ? injected.replace(/\/+$/, '') : '';
}

async function webFetch<T>(
  method: RouteSpec['method'],
  path: string,
  body: unknown,
  options?: CallApiOptions,
): Promise<T> {
  const headers: Record<string, string> = { 'content-type': 'application/json' };
  // 服务端以 `--password=…` / `SIMPLE_POST_WEB_PASSWORD` 启动时，渲染层的 `webFetch`
  // 必须给每次 API 调用附上 `Authorization: Bearer <token>`。bearer 缓存在
  // `localStorage`（见 `WEB_BEARER_STORAGE_KEY` 上方的说明）；新页面加载时 SPA shell
  // 先从 URL 里找 `?token=<x>` 并在任何 fetch 之前写入 localStorage。我们接受这个
  // XSS 暴露面 —— 与 Postman / Insomnia / dbx 同样的姿态，它们也都从 localStorage
  // 或扩展存储里读 token。
  const bearer = readBearerFromStorage();
  if (bearer) {
    headers['authorization'] = `Bearer ${bearer}`;
  }
  const init: RequestInit = {
    method,
    headers,
  };
  if (body !== null && method !== 'GET' && method !== 'DELETE') {
    init.body = JSON.stringify(body);
  }
  if (options?.signal) {
    init.signal = options.signal;
  }
  const res = await fetch(readApiBase() + path, init);
  if (!res.ok) {
    // 两个分支都是先赋值后使用 —— 声明时不带初始值，好让 `no-useless-assignment` 满意。
    let detail: string;
    try {
      const j = (await res.json()) as { code?: string; message?: string };
      detail = ` [${j.code ?? '?'}] ${j.message ?? ''}`;
    } catch {
      detail = ` ${await res.text().catch(() => '')}`;
    }
    throw new Error(`HTTP ${res.status} ${method} ${path}${detail}`);
  }
  // 204 No Content（DELETE、PUT-pref）→ 返回 undefined 并 cast 成 T。
  if (res.status === 204) return undefined as unknown as T;
  return (await res.json()) as T;
}
