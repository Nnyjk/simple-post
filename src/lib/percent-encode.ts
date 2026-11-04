import { wellFormed } from './well-formed';

/**
 * 百分号编码：字符集与 Rust 的 `core::url::percent_encode` 逐字节一致（保留
 * `A-Z a-z 0-9 - _ . ! ~ * ' ( )`，其余字节 `%XX`；空格是 `%20` 不是 `+`）。
 *
 * 落单的代理项先换成 U+FFFD 再编码，故本函数不抛 `URIError`。
 */
export function percentEncode(value: string): string {
  return encodeURIComponent(wellFormed(value));
}
