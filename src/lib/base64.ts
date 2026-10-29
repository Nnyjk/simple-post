/**
 * base64 与字节之间的转换 —— 传输层的二进制通道用它。
 *
 * 浏览器只有 `btoa` / `atob`，而它们吃的是「每个字符一个字节」的字符串。故字节与
 * 字符串之间要过一次 latin1 转换：直接 `btoa(String.fromCharCode(...bytes))` 在大文件
 * 上会炸调用栈，故分块处理。
 *
 * 字母表是标准的那个（`+` / `/`，带 `=` 补位），与 Rust 的
 * `base64::engine::general_purpose::STANDARD` 一致——两侧拼出来的字符串要能互换。
 */

/** 一次转换的块大小。太大会在 `String.fromCharCode` 上炸掉调用栈。 */
const CHUNK = 0x8000;

/** 字节 → base64（标准字母表，带补位）。 */
export function bytesToBase64(bytes: Uint8Array<ArrayBuffer>): string {
  let bin = '';
  for (let i = 0; i < bytes.length; i += CHUNK) {
    bin += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(bin);
}

/**
 * base64 → 字节。
 *
 * 解不开时给**空数组**：那份内容不是有效的 base64，而发一个内容被改写过的文件比发一个
 * 空文件更容易发现。
 */
export function base64ToBytes(base64: string): Uint8Array<ArrayBuffer> {
  let bin: string;
  try {
    bin = atob(base64);
  } catch {
    return new Uint8Array(0);
  }
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** 文本 → UTF-8 字节。 */
export function utf8Bytes(text: string): Uint8Array<ArrayBuffer> {
  return new TextEncoder().encode(text);
}

/**
 * 这串字节是合法 UTF-8 吗？
 *
 * 判据是「能不能用严格模式解成字符串」：严格模式遇到非法序列会抛，而宽松模式会把
 * 坏字节换成 U+FFFD——那等于把一份二进制悄悄改写成文本。
 *
 * `TextDecoder` 的 `fatal: true` 就是严格模式。
 */
export function isValidUtf8(bytes: Uint8Array<ArrayBuffer>): boolean {
  try {
    new TextDecoder('utf-8', { fatal: true }).decode(bytes);
    return true;
  } catch {
    return false;
  }
}

/** 字节 → 文本（调用方要保证字节是合法 UTF-8）。 */
export function bytesToUtf8(bytes: Uint8Array<ArrayBuffer>): string {
  return new TextDecoder('utf-8').decode(bytes);
}
