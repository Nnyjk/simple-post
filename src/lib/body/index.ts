/**
 * 请求体的解析与装配 —— 渲染进程与 MCP 端共用的那套规则。
 *
 * 与 `crates/simple-post-core/src/body.rs` 是同一套规则的两份实现，逐条对齐：
 * 模板化按字段做还是按整段做、空键与禁用行算不算、百分号编码用哪套字符集、产物
 * 是文本还是二进制。改动两边必须同步。
 *
 * ## 一个请求体有七种形态
 *
 * `BodyMode` 的每个变体在 `RequestBody.content` 里装的东西都不一样：
 *
 * | 模式 | `content` 装的是什么 | 产物 |
 * |---|---|---|
 * | `none` | 空 | 没有 body |
 * | `json` / `xml` / `raw` | 原文 | 文本 |
 * | `form` | 行数组的 JSON 文本 | 文本（urlencoded） |
 * | `multipart` | 分片数组的 JSON 文本 | 字节 |
 * | `binary` | 文件描述的 JSON 文本 | 字节（文件本身） |
 *
 * `form` 与 `multipart` 是**结构化**的：它们的 `content` 是一段 JSON 文本，描述的是
 * 一行一行 / 一个一个分片，而不是拼好的正文。结构化带来两样东西：界面上的表格有地方
 * 放说明，文件分片有地方放字节。
 *
 * ## 模板化按字段做，不按整段做
 *
 * `json` / `xml` / `raw` 整段替换；`form` 与 `multipart` **逐个字段**替换。整段替换在
 * 结构化形态上是错的：替换出来的值里带一个引号就会把那段 JSON 文本弄坏，于是整份内容
 * 解析不出来，请求发出去的是一个空 body。
 *
 * ## 产物的两种形态
 *
 * 产物是**文本**还是**二进制**，由字节决定：字节是合法 UTF-8 就走文本形态，否则走
 * base64 形态。`binary` 模式一律走二进制形态——用户选的是文件，发的是它的字节。
 */
import type { BodyFile, FormField, MultipartPart, RequestBody } from '@/types/domain';
import { resolveVarsWithMissing } from '../url';

/** 解析后的请求体。 */
export interface ResolvedBody {
  /** 文本形态。 */
  text: string | null;
  /** 二进制形态（base64）。 */
  base64: string | null;
  /** 没解析出来的变量名（去重，按首次出现排序）。 */
  missing: string[];
}

/** 没有请求体。 */
function emptyBody(): ResolvedBody {
  return { text: null, base64: null, missing: [] };
}

// -------------------------------------------------------------------------
// 按模式解析
// -------------------------------------------------------------------------

/**
 * 按 `RequestBody.mode` 解析请求体。
 *
 * `vars` 是这一次的变量池。`json` / `xml` / `raw` 整段替换，`form` 与 `multipart`
 * 逐个字段替换，`binary` 不替换（文件字节不是模板）。
 */
export function resolveBody(body: RequestBody, vars: Record<string, string>): ResolvedBody {
  if (body.mode === 'none' || body.content.length === 0) {
    return emptyBody();
  }

  switch (body.mode) {
    case 'json':
    case 'xml':
    case 'raw': {
      const r = resolveVarsWithMissing(body.content, vars);
      return { text: r.value, base64: null, missing: r.missing };
    }
    case 'form':
      return resolveFormBody(body.content, vars);
    case 'multipart':
      return resolveMultipartBody(body.content, vars);
    case 'binary':
      return resolveBinaryBody(body.content);
  }
}

/**
 * `form` 模式：逐行模板化，然后拼成 urlencoded 正文。
 *
 * **空键的行不进正文**，其中的变量也不报缺失——与查询参数同一条规则：没有名字的字段
 * 没有地方可去。
 */
function resolveFormBody(content: string, vars: Record<string, string>): ResolvedBody {
  const fields = parseFormFields(content);
  const missing: string[] = [];
  const pairs: string[] = [];

  for (const field of fields) {
    if (!field.enabled || field.key === '') {
      continue;
    }
    const key = resolveVarsWithMissing(field.key, vars);
    const value = resolveVarsWithMissing(field.value, vars);
    missing.push(...key.missing, ...value.missing);
    pairs.push(`${percentEncode(key.value)}=${percentEncode(value.value)}`);
  }

  return { text: pairs.join('&'), base64: null, missing: dedup(missing) };
}

/**
 * `multipart` 模式：逐个分片模板化，然后重新序列化成分片数组。
 *
 * 装配（boundary、分片头、字节）在发送侧做：boundary 每次现生成，装配与 Content-Type
 * 必须用同一个值。
 */
function resolveMultipartBody(content: string, vars: Record<string, string>): ResolvedBody {
  const parts = parseMultipartParts(content);
  const missing: string[] = [];

  const resolved = parts.map((part) => {
    // 文件分片的字节不模板化：base64 里没有 `{{`，模板化它只会平白多扫一遍。
    // 分片名也不模板化——它与文件名一起描述「哪个文件」，不是用户填的值。
    if (part.base64 !== undefined) return part;
    const name = resolveVarsWithMissing(part.name, vars);
    const value = resolveVarsWithMissing(part.value, vars);
    missing.push(...name.missing, ...value.missing);
    return { ...part, name: name.value, value: value.value };
  });

  return { text: JSON.stringify(resolved), base64: null, missing: dedup(missing) };
}

/**
 * `binary` 模式：文件描述里的字节原样带出去。
 *
 * 文件描述读不出来（内容不是那份 JSON）时没有 body：发一个空 body 好过发一段被改写
 * 过的文本。
 */
function resolveBinaryBody(content: string): ResolvedBody {
  const file = parseBodyFile(content);
  if (!file) return emptyBody();
  return { text: null, base64: file.base64, missing: [] };
}

// -------------------------------------------------------------------------
// 解析：form 的行
// -------------------------------------------------------------------------

/**
 * `form` 模式的内容 → 行。
 *
 * 内容是行数组的 JSON 文本。**解析不出来时给空数组**，不报错：这是一个纯函数，两条发送
 * 路径在这一点上必须同形。
 *
 * 任一行的字段类型不对就整份作废，与 multipart 分片的规则一致：逐行各自取值会让同一个
 * 畸形输入在两侧得到不同的正文。
 *
 * **不是行数组时按旧格式读**：早期版本的 `form` 模式存的是一个拼好的 urlencoded 串
 * （Postman 导入器至今还那么写）。那份内容不该被当成空表单——那等于把用户已经写好的
 * 字段悄悄丢掉。
 */
export function parseFormFields(content: string): FormField[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch {
    return parseLegacyFormFields(content);
  }
  if (!Array.isArray(parsed)) return parseLegacyFormFields(content);

  const fields: FormField[] = [];
  for (const raw of parsed) {
    if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return [];
    const r = raw as Record<string, unknown>;

    const key = r.key === undefined ? '' : str(r.key);
    const value = r.value === undefined ? '' : str(r.value);
    if (key === null || value === null) return [];

    // `enabled` 缺省为真：一份没写 `enabled` 的行就是一行要发的表单字段。
    const enabled = r.enabled === undefined || r.enabled === null ? true : bool(r.enabled);
    if (enabled === null) return [];

    // `description` 是 `Option`：缺省与 `null` 都是「没有说明」，类型不对才整份作废。
    let description: string | undefined;
    if (r.description !== undefined && r.description !== null) {
      const d = str(r.description);
      if (d === null) return [];
      description = d;
    }

    fields.push(
      description === undefined ? { key, value, enabled } : { key, value, enabled, description },
    );
  }
  return fields;
}

/**
 * 旧格式的 `form` 内容（urlencoded 串）→ 行。
 *
 * 按 `&` 分行，每行按**第一个** `=` 分成键与值。值不做百分号解码：那份内容里存的是
 * 用户写下的原文，解码会把值里的 `%` 变成别的字符。
 */
export function parseLegacyFormFields(content: string): FormField[] {
  if (!content) return [];
  return content
    .split('&')
    .filter((pair) => pair !== '')
    .map((pair) => {
      const eq = pair.indexOf('=');
      if (eq === -1) return { key: pair, value: '', enabled: true };
      return { key: pair.slice(0, eq), value: pair.slice(eq + 1), enabled: true };
    });
}

/** 行 → 行数组的 JSON 文本。 */
export function serializeFormFields(fields: FormField[]): string {
  return JSON.stringify(fields);
}

// -------------------------------------------------------------------------
// 解析：multipart 的分片
// -------------------------------------------------------------------------

/**
 * `multipart` 模式的内容 → 分片。
 *
 * **解析不出来时给空数组**，不报错——与 Rust 侧 `core::body::parse_multipart_parts`
 * 同一条规则。
 *
 * 任一分片的字段类型不对就整份作废，与 serde 反序列化失败后丢弃整份一致。
 */
export function parseMultipartParts(content: string): MultipartPart[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch {
    return [];
  }
  if (!Array.isArray(parsed)) return [];

  const parts: MultipartPart[] = [];
  for (const raw of parsed) {
    if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return [];
    const r = raw as Record<string, unknown>;

    const name = r.name === undefined ? '' : str(r.name);
    const value = r.value === undefined ? '' : str(r.value);
    if (name === null || value === null) return [];

    // `enabled` 缺省为真：一份没写 `enabled` 的分片就是一份要发的分片。
    const enabled = r.enabled === undefined || r.enabled === null ? true : bool(r.enabled);
    if (enabled === null) return [];

    const part: MultipartPart = { name, value, enabled };
    const filename = optStr(r.filename);
    if (filename === null) return [];
    if (filename !== undefined) part.filename = filename;
    const contentType = optStr(r.contentType);
    if (contentType === null) return [];
    if (contentType !== undefined) part.contentType = contentType;
    const base64 = optStr(r.base64);
    if (base64 === null) return [];
    if (base64 !== undefined) part.base64 = base64;
    const description = optStr(r.description);
    if (description === null) return [];
    if (description !== undefined) part.description = description;
    parts.push(part);
  }
  return parts;
}

// -------------------------------------------------------------------------
// 解析：binary 的文件
// -------------------------------------------------------------------------

/**
 * `binary` 模式的内容 → 文件描述。
 *
 * 读不出来时给 `null`：那个内容不是文件描述，就没有文件。
 */
export function parseBodyFile(content: string): BodyFile | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch {
    return null;
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) return null;
  const r = parsed as Record<string, unknown>;

  const name = r.name === undefined ? '' : str(r.name);
  if (name === null) return null;
  const contentType = r.contentType === undefined ? '' : str(r.contentType);
  if (contentType === null) return null;
  const base64 = r.base64 === undefined ? '' : str(r.base64);
  if (base64 === null) return null;
  // 字节数是非负整数：`-1` 与 `1.5` 都不是「文件有多少字节」。
  const size = r.size === undefined ? 0 : uint(r.size);
  if (size === null) return null;

  return { name, size, contentType, base64 };
}

/**
 * `binary` 模式请求体的 Content-Type：文件自己的 MIME 类型，缺省
 * `application/octet-stream`。
 *
 * 读不出文件描述时给缺省值：没有文件也就没有类型，而 `application/octet-stream`
 * 是任何服务端都能收下的形态。
 */
export function binaryContentType(content: string): string {
  const file = parseBodyFile(content);
  if (!file || !file.contentType) return 'application/octet-stream';
  return file.contentType;
}

// -------------------------------------------------------------------------
// 内部
// -------------------------------------------------------------------------

/**
 * 百分号编码。保留 `A-Z a-z 0-9 - _ . ! ~ * ' ( )`，其余字节一律 `%XX`（大写十六进制）。
 *
 * 字符集取 JS 的 `encodeURIComponent` 而不是 RFC 3986 的 unreserved 集：多出来的
 * `! * ' ( )` 在查询串里是合法的子分隔符，而渲染进程发出去的请求用的就是这套编码。
 * 空格是 `%20`（不是 form 编码的 `+`）：`+` 在严格按 RFC 3986 解析查询串的服务端
 * 会被读成加号而非空格。
 *
 * 这里按 UTF-8 **字节**编码，与 Rust 的 `core::url::percent_encode` 逐字节一致。
 */
export function percentEncode(s: string): string {
  const bytes = new TextEncoder().encode(s);
  let out = '';
  for (const b of bytes) {
    if (
      (b >= 0x41 && b <= 0x5a) ||
      (b >= 0x61 && b <= 0x7a) ||
      (b >= 0x30 && b <= 0x39) ||
      b === 0x2d ||
      b === 0x5f ||
      b === 0x2e ||
      b === 0x21 ||
      b === 0x7e ||
      b === 0x2a ||
      b === 0x27 ||
      b === 0x28 ||
      b === 0x29
    ) {
      out += String.fromCharCode(b);
    } else {
      out += '%' + b.toString(16).padStart(2, '0').toUpperCase();
    }
  }
  return out;
}

/** 去重并保持首次出现的次序。 */
function dedup(names: string[]): string[] {
  const seen = new Set<string>();
  return names.filter((name) => {
    if (seen.has(name)) return false;
    seen.add(name);
    return true;
  });
}

/** 字符串或 `null`（`null` 表示类型不对）。 */
function str(v: unknown): string | null {
  return typeof v === 'string' ? v : null;
}

/** 布尔值或 `null`（`null` 表示类型不对）。 */
function bool(v: unknown): boolean | null {
  return typeof v === 'boolean' ? v : null;
}

/** 非负整数或 `null`（`null` 表示类型不对）。 */
function uint(v: unknown): number | null {
  if (typeof v !== 'number' || !Number.isInteger(v) || v < 0) return null;
  return v;
}

/**
 * 可选字符串字段：缺省与 `null` 都给 `undefined`，类型不对给 `null`（调用方据此
 * 整份作废）。
 */
function optStr(v: unknown): string | undefined | null {
  if (v === undefined || v === null) return undefined;
  return typeof v === 'string' ? v : null;
}
