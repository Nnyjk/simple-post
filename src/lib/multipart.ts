/**
 * `multipart/form-data` 的分片装配 —— Content-Type 的 boundary 与 body 里的
 * boundary **必须是同一个字符串**，否则服务端解析失败而请求看起来成功。
 *
 * - 分片结构存在 `RequestBody.content` 里，是一个 `MultipartPart[]` 的 JSON 文本。复用
 *   既有的 `content` 字段（而不是给 `RequestBody` 加 `parts`）是有意的：`RequestBody`
 *   原样进 `body_json`，`{{var}}` 模板化也照旧作用在这个字符串上。
 * - **产物是字节**：文件分片的内容是文件的字节，文本分片的内容是 UTF-8 字节；字节是合法
 *   UTF-8 时走文本形态，否则走 base64 形态。
 * - 与 `mcp/http_ops/multipart.rs` 是同一套规则的两份实现（边界字符集、转义、缺省
 *   Content-Type、非法输入的处理都逐条对齐），改动两边必须同步。
 */
import type { MultipartPart } from '@/types/domain';
import { base64ToBytes, bytesToBase64, bytesToUtf8, isValidUtf8, utf8Bytes } from './base64';

export { parseMultipartParts } from './body';

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
 * 分片的内容字节。
 *
 * 文件分片的内容是 `base64` 里的字节；解不开时给空——那份内容不是有效的 base64，
 * 而发一个内容被改写过的文件比发一个空文件更容易发现。
 */
function partBytes(part: MultipartPart): Uint8Array<ArrayBuffer> {
  if (part.base64 !== undefined) return base64ToBytes(part.base64);
  return utf8Bytes(part.value);
}

/** 把若干段字节接成一段。 */
function concatBytes(chunks: readonly Uint8Array<ArrayBuffer>[]): Uint8Array<ArrayBuffer> {
  let total = 0;
  for (const c of chunks) total += c.length;
  const out = new Uint8Array(total);
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.length;
  }
  return out;
}

/**
 * 一个分片的内容在装配后请求体里的字节区间（左闭右开）。
 *
 * `index` 是它在**原始分片数组**里的下标，停用的分片不出现。渲染层据此把整体的上传 /
 * 接收字节数折算到具体那一行。
 */
export interface MultipartPartByteRange {
  index: number;
  start: number;
  end: number;
}

/** 装配时的一段字节，附带它来自哪个分片（框界为空）。 */
interface BodyChunk {
  bytes: Uint8Array<ArrayBuffer>;
  /** 分片在原始数组里的下标；分片之间的框界与收尾分隔符为 `null`。 */
  partIndex: number | null;
}

/**
 * 按 RFC 2046 的次序把分片与框界拼成一段段字节。**唯一的装配实现**：
 * `buildMultipartBody` 与 `multipartPartByteRanges` 都从它出发，故两者的偏移永远一致。
 */
function bodyChunks(parts: readonly MultipartPart[], boundary: string): BodyChunk[] {
  const chunks: BodyChunk[] = [];
  parts.forEach((part, index) => {
    // 停用的分片不进 body：它还留在表格里，用户改主意时勾回来即可。
    if (!part.enabled) return;
    let disposition = `Content-Disposition: form-data; name="${escapeHeaderParam(part.name)}"`;
    if (part.filename) disposition += `; filename="${escapeHeaderParam(part.filename)}"`;
    chunks.push({
      bytes: utf8Bytes(`--${boundary}\r\n${disposition}\r\n`),
      partIndex: null,
    });

    const contentType = partContentType(part);
    if (contentType) {
      chunks.push({ bytes: utf8Bytes(`Content-Type: ${contentType}\r\n`), partIndex: null });
    }

    chunks.push({ bytes: utf8Bytes('\r\n'), partIndex: null });
    chunks.push({ bytes: partBytes(part), partIndex: index });
    chunks.push({ bytes: utf8Bytes('\r\n'), partIndex: null });
  });
  chunks.push({ bytes: utf8Bytes(`--${boundary}--\r\n`), partIndex: null });
  return chunks;
}

/**
 * 分片数组 + boundary → 真正写进 socket 的 body。行尾是 CRLF（RFC 2046）。
 *
 * 零个分片得到 `--<boundary>--\r\n`，仍是格式合法的 multipart。
 */
export function buildMultipartBody(
  parts: readonly MultipartPart[],
  boundary: string,
): Uint8Array<ArrayBuffer> {
  return concatBytes(bodyChunks(parts, boundary).map((chunk) => chunk.bytes));
}

/**
 * 每个分片内容在装配后请求体里的字节区间。
 *
 * 与 `buildMultipartBody` 共用 `bodyChunks`，故窗口是按同一份字节算的：上传到 42% 时落在
 * 哪个文件分片里，两边不会有第二种答案。
 */
export function multipartPartByteRanges(
  parts: readonly MultipartPart[],
  boundary: string,
): MultipartPartByteRange[] {
  const ranges: MultipartPartByteRange[] = [];
  let at = 0;
  for (const chunk of bodyChunks(parts, boundary)) {
    const end = at + chunk.bytes.length;
    if (chunk.partIndex !== null) ranges.push({ index: chunk.partIndex, start: at, end });
    at = end;
  }
  return ranges;
}

/**
 * 装配好的 multipart 请求体。
 *
 * 文本形态与二进制形态**互斥**：字节是合法 UTF-8 时走文本形态，否则走 base64 形态。
 */
export interface AssembledBody {
  /** 文本形态。 */
  text: string | null;
  /** 二进制形态（base64）。 */
  base64: string | null;
}

/**
 * 分片数组 + boundary → 装配好的请求体。
 *
 * 产物是字节，而传输层要的是「文本或 base64」二者之一，故这里按字节是否合法 UTF-8
 * 选形态：合法的走文本（既有行为不变），不合法的走 base64（文件分片里通常有）。
 */
export function assembleMultipart(
  parts: readonly MultipartPart[],
  boundary: string,
): AssembledBody {
  const bytes = buildMultipartBody(parts, boundary);
  if (isValidUtf8(bytes)) {
    return { text: bytesToUtf8(bytes), base64: null };
  }
  return { text: null, base64: bytesToBase64(bytes) };
}
