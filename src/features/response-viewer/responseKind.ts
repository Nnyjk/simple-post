import type { ApiResponse } from '@/types/domain';
import { base64ToBytes, bytesToBase64, utf8Bytes } from '@/lib/base64';

/**
 * 响应内容的四种形态 —— 界面按它决定怎么渲染。
 *
 * - `json`：Content-Type 说它是 JSON，按 JSON 渲染（可格式化）。
 * - `text`：其它文本，原样渲染。
 * - `image`：图片，给出预览。
 * - `binary`：二进制文件，给出下载。
 *
 * **判据先看 Content-Type，再看字节。** 后端把响应读回来时会按体积上限裁过，裁点落在
 * 多字节字符中间时，一个纯文本响应读回来也是非法 UTF-8；只看字节会把文本误判成二进制。
 * 故后端先按 Content-Type 判「这是不是文本」，再按字节判「文本能不能原样解码」。
 */
export type ResponseKind = 'json' | 'text' | 'image' | 'binary';

/** 这个 Content-Type 说内容是 JSON 吗？ */
function isJsonContentType(contentType: string): boolean {
  const main = contentType.split(';')[0]?.trim().toLowerCase() ?? '';
  const slash = main.indexOf('/');
  if (slash === -1) return false;
  const subtype = main.slice(slash + 1);
  return subtype === 'json' || subtype.endsWith('+json');
}

/** 这个 Content-Type 说内容是图片吗？ */
function isImageContentType(contentType: string): boolean {
  const main = contentType.split(';')[0]?.trim().toLowerCase() ?? '';
  return main.startsWith('image/');
}

/**
 * 这个响应该怎么渲染。
 *
 * 图片先按 Content-Type 判：SVG 是文本（合法 UTF-8），后端会把它放进文本形态，而它
 * 仍然是一张图——只按字节判的话 SVG 会被当成一段普通文本。
 */
export function classifyResponse(response: ApiResponse): ResponseKind {
  const contentType = response.contentType ?? '';
  if (isImageContentType(contentType)) return 'image';
  // 有 base64 就说明后端判定它是二进制（文本形态不给 base64）。
  if (response.bodyBase64) return 'binary';
  if (isJsonContentType(contentType)) return 'json';
  return 'text';
}

/**
 * 响应体的原始字节。
 *
 * 后端给的是「文本形态」或「base64 形态」二者之一：二进制走 base64，文本走字符串。
 * 文本那份是宽松解码的结果，非法字节已被替换成 U+FFFD——故只有文本形态时才能说它
 * 就是原文（那时它本来就是合法 UTF-8）。
 */
export function responseBytes(response: ApiResponse): Uint8Array<ArrayBuffer> {
  if (response.bodyBase64) return base64ToBytes(response.bodyBase64);
  return utf8Bytes(response.body);
}

/** 内容类型到文件扩展名。取不到就用 `bin`。 */
const EXTENSION_BY_CONTENT_TYPE: Record<string, string> = {
  'application/json': 'json',
  'application/xml': 'xml',
  'application/pdf': 'pdf',
  'application/zip': 'zip',
  'application/gzip': 'gz',
  'application/octet-stream': 'bin',
  'application/x-tar': 'tar',
  'text/plain': 'txt',
  'text/html': 'html',
  'text/csv': 'csv',
  'image/png': 'png',
  'image/jpeg': 'jpg',
  'image/gif': 'gif',
  'image/webp': 'webp',
  'image/svg+xml': 'svg',
  'image/bmp': 'bmp',
  'image/x-icon': 'ico',
};

/**
 * 下载用的文件名。
 *
 * 先按 `Content-Disposition` 里服务端给的名字（那是服务端最清楚的文件名），取不到
 * 再按 Content-Type 猜扩展名。
 */
export function responseFileName(response: ApiResponse): string {
  const fromHeader = fileNameFromContentDisposition(response.headers['content-disposition']);
  if (fromHeader) return fromHeader;

  const contentType = (response.contentType ?? '').split(';')[0]?.trim().toLowerCase() ?? '';
  const ext = EXTENSION_BY_CONTENT_TYPE[contentType] ?? 'bin';
  return `response.${ext}`;
}

/**
 * 从 `Content-Disposition` 里取文件名。
 *
 * RFC 5987 的 `filename*=UTF-8''…` 优先：它能带非 ASCII 名字，而 `filename="…"` 那份
 * 按规范只能是 ASCII。
 */
function fileNameFromContentDisposition(header: string | undefined): string | null {
  if (!header) return null;

  const encoded = /filename\*\s*=\s*([^']*)'([^']*)'([^;]+)/i.exec(header);
  if (encoded) {
    try {
      const name = decodeURIComponent(encoded[3]!.trim().replace(/^"|"$/g, ''));
      if (name) return name;
    } catch {
      // 解不开就退回下面那份。
    }
  }

  const plain = /filename\s*=\s*("([^"]*)"|[^;]+)/i.exec(header);
  if (plain) {
    const name = (plain[2] ?? plain[1] ?? '').trim();
    if (name) return name;
  }
  return null;
}

/**
 * 把响应存成文件。
 *
 * 存的是**原始字节**：文本形态存 UTF-8 编码，二进制形态存 base64 解出来的字节。
 * 用 `text/plain` 存二进制会把字节改写成乱码。
 */
export function downloadResponse(response: ApiResponse): void {
  const bytes = responseBytes(response);
  const blob = new Blob([bytes], { type: response.contentType || 'application/octet-stream' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = responseFileName(response);
  a.click();
  URL.revokeObjectURL(url);
}

/**
 * 图片预览用的 data URL。
 *
 * 文本形态的响应（SVG 这类）按 UTF-8 重新编码成 base64——那份文本本来就是合法 UTF-8，
 * 重新编码得到的字节与原始字节一致。
 */
export function responseDataUrl(response: ApiResponse): string | null {
  if (!isImageContentType(response.contentType ?? '')) return null;
  const bytes = responseBytes(response);
  const contentType = (response.contentType ?? '').split(';')[0]?.trim() || 'image/*';
  return `data:${contentType};base64,${bytesToBase64(bytes)}`;
}
