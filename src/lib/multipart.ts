/**
 * `multipart/form-data` 的分片装配 —— Content-Type 的 boundary 与 body 里的
 * boundary **必须是同一个字符串**，否则服务端解析失败而请求看起来成功。
 *
 * 分片结构存在 `RequestBody.content` 里，是一个 `MultipartPart[]` 的 JSON 文本。
 * 复用既有的 `content` 字段（而不是给 `RequestBody` 加 `parts`）是有意的：
 * `RequestBody` 原样进 `body_json`，`{{var}}` 模板化也照旧作用在这个字符串上。
 *
 * 与 `mcp/http_ops/multipart.rs` 是同一套规则的两份实现（边界字符集、转义、
 * 缺省 Content-Type、非法输入的处理都逐条对齐），改动两边必须同步。
 */
import type { MultipartPart } from '@/types/domain';

/** 文件分片没写 `contentType` 时的缺省值，与浏览器发 FormData 一致。 */
const DEFAULT_FILE_CONTENT_TYPE = 'application/octet-stream';

/**
 * 现生成一个 boundary。RFC 2046 限长 70 且不允许出现在分片内容里，所以用
 * 固定前缀 + 随机十六进制。
 *
 * **每次请求现生成**：两条发送路径（UI / MCP）各生成各的，只要各自的 body 与
 * Content-Type 用的是同一个值即可，不需要跨路径一致。
 */
export function newMultipartBoundary(): string {
  const bytes = new Uint8Array(12);
  const source = globalThis.crypto;
  if (source && typeof source.getRandomValues === 'function') {
    source.getRandomValues(bytes);
  } else {
    for (let i = 0; i < bytes.length; i++) bytes[i] = Math.floor(Math.random() * 256);
  }
  let hex = '';
  for (const b of bytes) hex += b.toString(16).padStart(2, '0');
  return `----SimplePostFormBoundary${hex}`;
}

/** 带 boundary 的 Content-Type。 */
export function multipartContentType(boundary: string): string {
  return `multipart/form-data; boundary=${boundary}`;
}

function str(v: unknown): string | null {
  return typeof v === 'string' ? v : null;
}

/**
 * `RequestBody.content` 的 JSON 文本 → 分片数组。
 *
 * **非法输入一律返回空数组**，不发 `AppError`、不抛异常：这是纯函数且没有错误
 * 通道，两条发送路径在这一点上必须同形。空分片仍然是一个格式合法的 multipart body
 * （只有收尾分隔符），服务端会报「没有分片」——比发一个内容被静默改写的 body 更容易发现。
 *
 * 任一分片的字段类型不对就整份作废，与 Rust 侧 serde 反序列化失败后丢弃整份一致；
 * 逐字段兜底会让同一个畸形输入在两侧得到不同的分片。
 *
 * **显式 `null` 视为缺省**，与 Rust 侧 serde 的 `Option` 语义对齐：`#[serde(default)]`
 * 只覆盖缺键，JSON `null` 反序列化成 `None`——分片留着，只是那个字段没有值。
 * 两侧对 `null` 的判断若不同，同一个 body 会在 MCP 与 UI 上发成两份。
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

    const part: MultipartPart = { name, value };
    if (r.filename !== undefined && r.filename !== null) {
      const filename = str(r.filename);
      if (filename === null) return [];
      part.filename = filename;
    }
    if (r.contentType !== undefined && r.contentType !== null) {
      const contentType = str(r.contentType);
      if (contentType === null) return [];
      part.contentType = contentType;
    }
    parts.push(part);
  }
  return parts;
}

/**
 * 头参数里的引号与反斜杠要转义，而 CR/LF 必须整个去掉——留着就是往响应头里
 * 注入一个新头，服务端看到的分片名与用户填的不是一回事。
 */
function escapeHeaderParam(value: string): string {
  return value
    .replace(/[\r\n]/g, '')
    .replace(/\\/g, '\\\\')
    .replace(/"/g, '\\"');
}

/** 分片自己的 Content-Type：写了就用，没写且是文件分片则回落到二进制流类型。 */
function partContentType(part: MultipartPart): string | null {
  if (part.contentType) return part.contentType;
  if (part.filename) return DEFAULT_FILE_CONTENT_TYPE;
  return null;
}

/**
 * 分片数组 + boundary → 真正写进 socket 的 body。行尾是 CRLF（RFC 2046）。
 *
 * 零个分片得到 `--<boundary>--\r\n`，仍是格式合法的 multipart。
 */
export function buildMultipartBody(parts: readonly MultipartPart[], boundary: string): string {
  let out = '';
  for (const part of parts) {
    let disposition = `Content-Disposition: form-data; name="${escapeHeaderParam(part.name)}"`;
    if (part.filename) disposition += `; filename="${escapeHeaderParam(part.filename)}"`;
    out += `--${boundary}\r\n${disposition}\r\n`;

    const contentType = partContentType(part);
    if (contentType) out += `Content-Type: ${contentType}\r\n`;

    out += `\r\n${part.value}\r\n`;
  }
  return `${out}--${boundary}--\r\n`;
}
