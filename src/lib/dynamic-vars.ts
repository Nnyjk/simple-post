/**
 * 动态变量在前端这一侧的规则镜像。
 *
 * 取数、抽取与序列化都在后端（Rust 的 `core::dynamic`）完成，前端只负责两件事：
 *
 * 1. 把后端取回的值并进变量池（合并规则）；
 * 2. 找出请求模板里引用了哪些动态变量（名字扫描）。
 *
 * 这两条规则两侧各写一遍，故由 `fixtures/dynamic-parity.json` 钉住：Rust 的
 * `crates/simple-post-core/src/dynamic/resolve_parity_test.rs` 与
 * `src/lib/__tests__/dynamic-parity.test.ts` 读同一份文件，产出必须逐条相同。
 *
 * 值的序列化在后端发生（前端拿到的已经是替换用的文本），这里的 [`serializeValue`] 是
 * 同一套规则的镜像，供面板显示「保存后长什么样」与样本对照使用。
 */
import type { AuthConfig, KeyValue, RequestBody } from '@/types/domain';
import { listVars } from './url';

/** 值的序列化形态。与 Rust 的 `domain::ValueMode` 一一对应。 */
export type ValueMode = 'auto' | 'json' | 'text';

/** 请求模板的各段。扫描「哪些 `{{name}}` 被引用了」只需要这些。 */
export interface DynamicTemplateParts {
  /** base URL。它可能自身带变量，故与 path 一起扫。 */
  baseUrl: string;
  /**
   * 那一格的实际访问地址。它会被 `{{var}}` 替换，故它里面的标记同样算被引用 ——
   * 不扫的话，只在代理地址里出现的动态变量永远不会被取数，替换后仍是字面量 `{{name}}`。
   */
  proxy?: string;
  /** 端点的 path。 */
  url: string;
  params: KeyValue[];
  /**
   * URL 路径里的占位符与它们各自的值。它们替换进路径本身。
   *
   * 值里的 `{{var}}` 照样会被替换（见 `prepareRequest`），故它们里的标记同样算被
   * 引用 —— 不扫的话，只在路径参数值里出现的动态变量永远不会被取数，替换后仍是
   * 字面量 `{{name}}`。
   */
  pathParams?: KeyValue[];
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
}

/**
 * 合成变量池：环境变量（去掉停用的那些）+ 动态变量的值。
 *
 * 两条子规则都由「动态变量放在停用过滤之后并入」这一条实现方式成立：
 *
 * 1. 动态变量的值覆盖同名环境变量——它是后面并进去的；
 * 2. 被停用的环境变量不影响动态变量——停用过滤只作用于环境变量。
 */
export function mergePool(
  envVars: Record<string, string>,
  disabled: string[],
  dynamicVars: Record<string, string>,
): Record<string, string> {
  const pool: Record<string, string> = {};
  for (const [key, value] of Object.entries(envVars)) {
    if (disabled.includes(key)) continue;
    pool[key] = value;
  }
  for (const [key, value] of Object.entries(dynamicVars)) {
    pool[key] = value;
  }
  return pool;
}

/**
 * 模板里出现的全部 `{{name}}` 名字，按首次出现排序。
 *
 * 只扫**会被替换**的字段：启用的且 key 非空的参数值、启用的且 key 非空的路径参数值、
 * 启用的且 key 非空的 header 值、body 内容、那一格的实际访问地址（代理地址），以及
 * 鉴权里那三种方案（bearer / basic / apikey）。OAuth2 的配置字段不被变量替换，故不在
 * 扫描范围内——扫了会让一条根本没被用到的定义去取数。
 */
export function listDynamicNames(parts: DynamicTemplateParts): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  const add = (text: string): void => {
    for (const name of listVars(text)) {
      if (seen.has(name)) continue;
      seen.add(name);
      out.push(name);
    }
  };

  add(parts.baseUrl);
  // 代理地址（那一格的实际访问地址）会被单独替换一遍，故它里面的标记同样算被引用。
  if (parts.proxy) add(parts.proxy);
  add(parts.url);
  for (const param of parts.params) {
    if (!param.enabled || !param.key) continue;
    add(param.value);
  }
  for (const param of parts.pathParams ?? []) {
    if (!param.enabled || !param.key) continue;
    add(param.value);
  }
  for (const header of parts.headers) {
    if (!header.enabled || !header.key) continue;
    add(header.value);
  }
  if (parts.body.content) add(parts.body.content);
  if (parts.auth.kind === 'bearer' && parts.auth.bearer) add(parts.auth.bearer);
  if (parts.auth.kind === 'basic' && parts.auth.basic) {
    add(parts.auth.basic.username);
    add(parts.auth.basic.password);
  }
  if (parts.auth.kind === 'apikey' && parts.auth.apikey) {
    add(parts.auth.apikey.key);
    add(parts.auth.apikey.value);
  }
  return out;
}

/**
 * 把抽到的值变成替换用的文本。
 *
 * - `json`：始终是 JSON 序列化，故标量带引号（`"A001"`）。
 * - `auto` 与 `text`：对象与数组给裸 JSON 片段，标量给纯文本。两者输出**完全一致**——
 *   `text` 是用户的声明（值会被放进请求体的字符串位置，转义由用户承担），不是另一种形态。
 *
 * 与 Rust 的 `core::dynamic::serialize_value` 同一套规则。唯一可能不同的地方是小数：
 * Rust 的 `serde_json::Number` 保留 `1.0` 的写法，JavaScript 的 `JSON.parse` 把它归一成
 * `1`。生产路径上序列化只发生在 Rust 侧，故这个差异不会影响真正发出的请求。
 */
export function serializeValue(value: unknown, mode: ValueMode): string {
  if (mode === 'json') return JSON.stringify(value) ?? '';
  if (typeof value === 'string') return value;
  if (typeof value === 'number') return String(value);
  if (typeof value === 'boolean') return String(value);
  if (value === null || value === undefined) return 'null';
  return JSON.stringify(value) ?? '';
}

/**
 * 覆盖值是用户手打的文本：能解析成 JSON 就用它，解析不了就当成一个字符串。
 *
 * 与 Rust 的 `core::dynamic::parse_raw` 同一套规则。
 */
export function parseRaw(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}
