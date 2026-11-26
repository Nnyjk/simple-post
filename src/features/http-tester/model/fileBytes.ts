import { bytesToBase64 } from '@/lib/base64';

/**
 * 读本机文件 —— 请求体的文件分片与 binary 模式都用它。
 *
 * **文件不进库，存的是字节。** 桌面端与服务端容器不是一个文件系统，存路径在服务端那一侧
 * 解不开；故 body 里存的是 base64 的字节本身。大文件会让 body 变得很大（base64 比原字节大
 * 三分之一），而 SQLite 的行与传输层的 JSON 都要背着它走。
 */

/** 一次读多少字节。192 KiB 是 3 的倍数，故中间每一块单独编码都不带 `=` 补位。 */
const READ_CHUNK_BYTES = 3 * 64 * 1024;

/** 「把资源添加到请求」这一步的进度：已读字节 / 文件总字节。 */
export interface ReadFileProgress {
  loaded: number;
  total: number;
}

/** `readFileAsBase64` 的可选参数。 */
export interface ReadFileOptions {
  /** 每读完一块回调一次；空文件也会收到一次 `0 / 0`。 */
  onProgress?: (progress: ReadFileProgress) => void;
  /** 中止读取。已读的块不会写回调用方，`arrayBuffer()` 返回后立刻抛 `AbortError`。 */
  signal?: AbortSignal;
  /** 覆盖一次读的字节数。任意值都正确：跨块的 base64 补位由本函数处理。 */
  chunkSize?: number;
}

/**
 * 读一个本地文件 → base64 的字节，逐块读并报告进度。
 *
 * 分块读有两样用处：进度条能显示到哪了，以及中途可以被 `signal` 取消。字节本身仍要全部
 * 进内存（base64 是最终形态），故这里不承诺省内存。
 */
export async function readFileAsBase64(file: File, options: ReadFileOptions = {}): Promise<string> {
  const { onProgress, signal } = options;
  const step = Math.max(1, Math.floor(options.chunkSize ?? READ_CHUNK_BYTES));
  const total = file.size;
  onProgress?.({ loaded: 0, total });

  let base64 = '';
  let offset = 0;
  // 不足 3 字节的尾巴留到下一块：base64 的 4 字符一组不能跨块拆开。
  let carry = new Uint8Array(0);
  while (offset < total) {
    if (signal?.aborted) throw new DOMException('The read was aborted', 'AbortError');
    const end = Math.min(offset + step, total);
    const chunk = new Uint8Array(await file.slice(offset, end).arrayBuffer());
    offset = end;

    const merged = new Uint8Array(carry.length + chunk.length);
    merged.set(carry, 0);
    merged.set(chunk, carry.length);
    const usable = merged.length - (merged.length % 3);
    if (usable > 0) base64 += bytesToBase64(merged.subarray(0, usable));
    carry = merged.slice(usable);

    onProgress?.({ loaded: offset, total });
  }
  if (carry.length > 0) base64 += bytesToBase64(carry);
  return base64;
}

/**
 * base64 字符串解码后的原始字节数，不看内容。
 *
 * 表格里的文件行要显示体积，而重新 `atob` 一遍只为数长度会把大文件再解一次。补位字符
 * 决定尾部，其余每个字符 6 位。
 */
export function decodedBase64ByteLength(base64: string): number {
  if (!base64) return 0;
  const padding = base64.endsWith('==') ? 2 : base64.endsWith('=') ? 1 : 0;
  return Math.max(0, Math.floor((base64.length * 3) / 4) - padding);
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
