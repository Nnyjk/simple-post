import { bytesToBase64 } from '@/lib/base64';

/**
 * 读本机文件 —— 请求体的文件分片与 binary 模式都用它。
 *
 * **文件不进库，存的是字节。** 桌面端与服务端容器不是一个文件系统，存路径在服务端
 * 那一侧解不开；故 body 里存的是 base64 的字节本身。
 *
 * 大文件会让 body 变得很大（base64 比原字节大三分之一），而 SQLite 的行与传输层的
 * JSON 都要背着它走。这是「发一个文件」这件事在纯客户端工具里的固有代价：文件字节
 * 必须跟着请求一起走。
 */

/** 读一个本地文件 → base64 的字节。 */
export async function readFileAsBase64(file: File): Promise<string> {
  const buffer = await file.arrayBuffer();
  return bytesToBase64(new Uint8Array(buffer));
}

/** 扩展名 → MIME 类型。浏览器给不出类型时按扩展名猜。 */
const CONTENT_TYPE_BY_EXTENSION: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  bmp: 'image/bmp',
  ico: 'image/x-icon',
  svg: 'image/svg+xml',
  pdf: 'application/pdf',
  zip: 'application/zip',
  gz: 'application/gzip',
  tar: 'application/x-tar',
  json: 'application/json',
  xml: 'application/xml',
  txt: 'text/plain',
  csv: 'text/csv',
  html: 'text/html',
  yaml: 'application/yaml',
  yml: 'application/yaml',
};

/** 文件的 MIME 类型。浏览器给不出时按扩展名猜。 */
export function fileContentType(file: File): string {
  if (file.type) return file.type;
  const ext = file.name.split('.').pop()?.toLowerCase() ?? '';
  return CONTENT_TYPE_BY_EXTENSION[ext] ?? 'application/octet-stream';
}
