/**
 * URL 路径里的占位符（`/users/{id}`）—— 识别与替换的原语。
 *
 * ## 它与 `{{var}}` 是两套语法
 *
 * `{{var}}` 取自变量池（环境变量与动态变量），`{id}` 取自端点自己的路径参数。
 * 两者住在同一段 URL 文本里，故识别占位符时必须先把 `{{…}}` 整段跳过——否则
 * `{{id}}` 会被当成名为 `{id` 的占位符，路径参数与变量池就串了。
 *
 * ## 只在「路径段」里找
 *
 * 占位符只出现在 URL 的路径段：第一个 `?` 或 `#` 之前、主机名之后的那一段。
 * 查询串里的 `{` 是数据（`?filter={status}`），主机名里的 `{` 是别的东西
 * （`http://{tenant}.example.com` 属于主机模板，不归路径参数管）。
 *
 * 对应 Rust 侧的 `crates/simple-post-core/src/path_params.rs`；两侧的规则由
 * `fixtures/resolve-parity.json` 里的路径参数用例钉住。
 */

import type { KeyValue } from '@/types/domain';
import { uid } from '@/lib/utils';

/** 一个占位符：名字，以及它在路径段里的起止下标（含两头大括号）。 */
interface Found {
  name: string;
  from: number;
  to: number;
}

/**
 * URL 文本里「路径段」的下标范围：起点在主机名之后，终点在第一个 `?` 或 `#` 之前。
 *
 * 没有 `://` 的文本（相对路径）整段都是路径段。
 */
function pathScope(url: string): { start: number; end: number } {
  const q = url.search(/[?#]/);
  const end = q === -1 ? url.length : q;
  const scheme = url.indexOf('://');
  if (scheme === -1) return { start: 0, end };
  const afterScheme = scheme + 3;
  if (afterScheme >= end) {
    // `https://?x` 这种形状：主机名后面没有路径段。
    return { start: end, end };
  }
  const slash = url.indexOf('/', afterScheme);
  // `https://example.com` 后面没有 `/`：整条 URL 就是主机名。
  return { start: slash === -1 || slash > end ? end : slash, end };
}

/**
 * 占位符名里不许有的空白：Unicode 的 White_Space 属性。
 *
 * 显式写出码点，而不是用 `/\s/`：JS 的 `\s` 含 U+FEFF、不含 U+0085，而 Rust 的
 * `char::is_whitespace` 正好相反。两边用不同的空白集合，同一段 URL 在界面与 MCP 侧
 * 就会认出不同的占位符名单 —— 差集见 `fixtures/path-params-parity.json` 里的那几条。
 */
const WHITESPACE = /[\t\n\v\f\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]/;

/**
 * 占位符名是否合法：非空，且不含空白与 `{ } / ? # &`。
 *
 * 斜杠被排除，因为一个路径参数占的是**一段**路径；值里的斜杠由编码处理
 * （`%2F`），写进模板会让路径多出一段。
 */
function isPlaceholderName(name: string): boolean {
  if (!name) return false;
  return [...name].every((c) => !WHITESPACE.test(c) && !'{} /?#&'.includes(c));
}

/**
 * 扫出路径段里的全部占位符，按出现顺序排列（同名重复出现会各占一条）。
 *
 * 扫描器不递归：`{{var}}` 整段跳过，单个 `{name}` 只吃到它自己的那个 `}`。
 */
function scan(scope: string): Found[] {
  const out: Found[] = [];
  let i = 0;
  while (i < scope.length) {
    if (scope[i] !== '{') {
      i += 1;
      continue;
    }
    // `{{` 是环境变量的写法：整段跳过，不把它当成路径占位符。
    if (scope[i + 1] === '{') {
      const close = scope.indexOf('}}', i + 2);
      if (close !== -1) {
        i = close + 2;
        continue;
      }
      i += 1;
      continue;
    }
    // 单个大括号：找它配对的那个 `}`。
    const close = scope.indexOf('}', i + 1);
    if (close === -1) {
      i += 1;
      continue;
    }
    const name = scope.slice(i + 1, close);
    if (isPlaceholderName(name)) {
      out.push({ name, from: i, to: close + 1 });
    }
    i = close + 1;
  }
  return out;
}

/**
 * 列出 URL 路径段里的占位符名，按首次出现去重。
 *
 * 界面用它列出「这个接口有哪几个路径参数要填」，导入导出侧用它判断
 * 「路径里的占位符有没有人认领」。
 */
export function pathPlaceholders(url: string): string[] {
  if (!url) return [];
  const { start, end } = pathScope(url);
  const scope = url.slice(start, end);
  const out: string[] = [];
  const seen = new Set<string>();
  for (const f of scan(scope)) {
    if (seen.has(f.name)) continue;
    seen.add(f.name);
    out.push(f.name);
  }
  return out;
}

/**
 * 写一个占位符的值，并按 URL 里的占位符顺序重排整份路径参数。
 *
 * `items` 是已存的路径参数（只有填过值的占位符才在里面），`placeholders` 是当前 URL
 * 里认得出的占位符名。返回值的顺序是「URL 里的占位符顺序 + 落单的存有条目」，于是
 * 用户改 URL 里的占位符名字时，列表里的行也跟着挪位。
 *
 * 三条规则：
 *
 * 1. **只有被改的那一个**占位符拿到新值，其余占位符沿用各自存着的值。把新值套到
 *    每一行上会让改一个参数顺手改掉另一个参数的值，而界面上看不出是谁被改了。
 * 2. **被改的占位符还没有存有条目时新建一条**。新建接口的用户第一次填 `{id}` 时，
 *    存储里并没有 `id` 这一条；若这里只更新已有条目，用户的按键会被回滚，而界面上
 *    看不出任何异常 —— 那一行的输入框永远写不进东西。
 * 3. **值被清空时把条目删掉**：「只有填过值的占位符才进存储」，空值留在存储里只会
 *    让「有没有填」这件事在两处说法不一。
 *
 * 落单的存有条目（在当前 URL 里找不到对应占位符的那些）原样保留在末尾：它们不进请求，
 * 但静默丢掉的话，用户改过占位符名字之后那些值就再也找不回来了。
 */
export function applyPathParamValue(
  items: readonly KeyValue[],
  placeholders: readonly string[],
  name: string,
  value: string,
): KeyValue[] {
  const next: KeyValue[] = [];
  for (const p of placeholders) {
    const existing = items.find((it) => it.key === p);
    if (p === name) {
      if (value.trim() === '') continue;
      next.push(existing ? { ...existing, value } : { id: uid(), key: p, value, enabled: true });
      continue;
    }
    if (!existing) continue;
    next.push(existing);
  }
  for (const it of items) {
    if (placeholders.includes(it.key)) continue;
    next.push(it);
  }
  return next;
}

/**
 * 把 URL 路径段里的占位符替换成给定的值。
 *
 * `values` 是「占位符名 → 取到的值」，值是**未经编码**的原文：编码由本函数统一做，
 * 字符集取 `encodeURIComponent`，与 Rust 侧 `simple_post_core::url::percent_encode`
 * 同一套，故路径参数的值与查询参数的值遵守同一条编码纪律。
 *
 * 取不到值的占位符**原样留在 URL 里**，并出现在返回的 `missing` 里。值是空串或只有
 * 空白时同样按「取不到」处理：路径参数占的是一段路径，替换成空串只会把路径截断，
 * 而一个只有空白的值是用户没填完，不是用户想发一个空格。
 *
 * 返回值第一项是替换后的 URL，第二项是取不到值的占位符名（按首次出现去重）。
 */
export function substitutePathParams(
  url: string,
  values: ReadonlyArray<readonly [string, string]>,
): { url: string; missing: string[] } {
  if (!url) return { url: '', missing: [] };
  const { start, end } = pathScope(url);
  const scope = url.slice(start, end);
  const found = scan(scope);
  if (found.length === 0) return { url, missing: [] };

  const missing: string[] = [];
  const seen = new Set<string>();
  let out = url.slice(0, start);
  let cursor = 0;
  for (const f of found) {
    out += scope.slice(cursor, f.from);
    const hit = values.find(([k]) => k === f.name);
    if (hit && hit[1].trim() !== '') {
      out += encodeURIComponent(hit[1]);
    } else {
      if (!seen.has(f.name)) {
        seen.add(f.name);
        missing.push(f.name);
      }
      // 取不到值就把占位符原样放回去，用户在 URL 预览里看得见缺哪个。
      out += scope.slice(f.from, f.to);
    }
    cursor = f.to;
  }
  out += scope.slice(cursor) + url.slice(end);
  return { url: out, missing };
}
