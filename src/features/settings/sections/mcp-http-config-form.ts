import type { McpHttpConfig } from '@/types/domain';

/**
 * Streamable-HTTP 监听表单的纯表单逻辑。
 *
 * 刻意与组件分开：本文件没有 React、没有 IPC，故那些解析规则（白名单切分、端口边界、
 * 「远程访问需要两份白名单」）只靠 `tsx --test` 就能测 —— 本仓库没有 DOM 测试环境
 * （AGENTS.md）。同样的切分在本目录里已有先例，即 `env-utils.tsx`。
 */

/** 服务端默认值。镜像 Rust 侧，保证首屏不会是空白。 */
export const DEFAULT_MCP_HTTP_CONFIG: McpHttpConfig = {
  host: '127.0.0.1',
  port: 0,
  path: '/mcp',
  allowRemote: false,
  allowedHosts: [],
  allowedOrigins: [],
};

export const MAX_PORT = 65535;

/**
 * 表单的工作态。有两个字段刻意不同于 `McpHttpConfig`：
 *
 * - `port` 是字符串：数字输入框编辑中途仍会回传 `''`，按数字存会让「清空」无从表达。
 * - 白名单是文本域的原始文本，这样打了一半的条目不会在解析成数组的往返中被丢掉。
 */
export interface McpHttpConfigDraft {
  host: string;
  port: string;
  path: string;
  allowRemote: boolean;
  /** 每行一条，也接受逗号分隔。 */
  allowedHosts: string;
  /** 每行一条，也接受逗号分隔。 */
  allowedOrigins: string;
}

/**
 * 把白名单文本域切分成条目。
 *
 * 换行与逗号都算分隔符，因为两种都是人会粘贴的东西（从代理配置里复制的 header 列表是
 * 逗号连起来的，而文本域是一行一条地编辑）。重复项合并，免得同一份列表每重打一遍就
 * 翻一次「已修改」。
 */
export function parseList(text: string): string[] {
  const out: string[] = [];
  for (const raw of text.split(/[\n,]/)) {
    const value = raw.trim();
    if (value === '') continue;
    if (!out.includes(value)) out.push(value);
  }
  return out;
}

/** 把白名单数组渲染回一行一条的文本域形态。 */
export function formatList(list: readonly string[]): string {
  return list.join('\n');
}

/** 由已持久化的配置播种一份草稿。 */
export function draftFromConfig(config: McpHttpConfig): McpHttpConfigDraft {
  return {
    host: config.host,
    port: String(config.port),
    path: config.path,
    allowRemote: config.allowRemote,
    allowedHosts: formatList(config.allowedHosts),
    allowedOrigins: formatList(config.allowedOrigins),
  };
}

/**
 * 解析端口字段。`null` 表示「不是可用端口」，也正是校验器会报出去的东西。
 *
 * 这条正则拒得比 `Number()` 更严：`'-1'`、`'1.5'`、`'0x10'` 以及 `' 12 '` 里夹着杂物的
 * 变体都可能被 `Number()` 以某种形式放过，而我们要的是非负整数。
 */
export function parsePort(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const value = Number(trimmed);
  if (!Number.isInteger(value) || value < 0 || value > MAX_PORT) return null;
  return value;
}

/** 强制补上前导斜杠；字段为空则回落到 `/`。 */
export function normalizePath(text: string): string {
  const trimmed = text.trim();
  if (trimmed === '') return '/';
  return trimmed.startsWith('/') ? trimmed : `/${trimmed}`;
}

/** 草稿 → 线上的配置；端口字段不可用时返回 `null`。 */
export function configFromDraft(draft: McpHttpConfigDraft): McpHttpConfig | null {
  const port = parsePort(draft.port);
  if (port === null) return null;
  return {
    host: draft.host.trim() === '' ? DEFAULT_MCP_HTTP_CONFIG.host : draft.host.trim(),
    port,
    path: normalizePath(draft.path),
    allowRemote: draft.allowRemote,
    allowedHosts: parseList(draft.allowedHosts),
    allowedOrigins: parseList(draft.allowedOrigins),
  };
}

/** 逐字段相等 —— 白名单不看顺序。 */
export function configEquals(a: McpHttpConfig, b: McpHttpConfig): boolean {
  const same = (x: readonly string[], y: readonly string[]) => {
    const sx = [...x].sort();
    const sy = [...y].sort();
    return sx.length === sy.length && sx.every((v, i) => v === sy[i]);
  };
  return (
    a.host === b.host &&
    a.port === b.port &&
    a.path === b.path &&
    a.allowRemote === b.allowRemote &&
    same(a.allowedHosts, b.allowedHosts) &&
    same(a.allowedOrigins, b.allowedOrigins)
  );
}

/**
 * 「有未保存修改」标志。
 *
 * 解析不出来的草稿算脏而非算干净：用户确实输入了东西，在一个表单自己都判定为非法的
 * 字段旁边显示「无修改」，就等于让保存悄悄丢掉他的编辑。
 */
export function isDraftDirty(draft: McpHttpConfigDraft, saved: McpHttpConfig): boolean {
  const next = configFromDraft(draft);
  if (next === null) return true;
  return !configEquals(next, saved);
}

/** `localhost` / `127.0.0.1` / `::1` —— 不需要白名单的地址。 */
export function isLoopbackHost(host: string): boolean {
  const value = host.trim().toLowerCase();
  return value === '127.0.0.1' || value === '::1' || value === 'localhost';
}

/** 该 Origin 条目是否带协议？`https://…` 带，`example.com` 不带。 */
function hasScheme(origin: string): boolean {
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(origin);
}

/** 当前草稿的阻塞性问题；可保存时为 `null`。顺序保证报错指向用户正在看的那个字段。 */
export function draftError(draft: McpHttpConfigDraft): string | null {
  if (parsePort(draft.port) === null) {
    return `端口需为 0–${MAX_PORT} 的整数，0 表示由系统分配`;
  }
  if (draft.allowRemote) {
    const hosts = parseList(draft.allowedHosts);
    const origins = parseList(draft.allowedOrigins);
    if (hosts.length === 0) return '开启远程访问后，至少需要一个 Host 白名单条目';
    if (origins.length === 0) return '开启远程访问后，至少需要一个 Origin 白名单条目';
    // `*` 是文档里写明的「不做检查」逃生口，故豁免。
    if (!hosts.includes('*') && !origins.includes('*')) {
      const bare = origins.filter((o) => !hasScheme(o));
      if (bare.length > 0) return `Origin 必须带协议，例如 https://${bare[0]}`;
    }
  }
  return null;
}
