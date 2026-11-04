import { wellFormed } from './well-formed';

/**
 * 百分号编码 —— 字符集与 Rust 的 `core::url::percent_encode` 逐字节一致。
 *
 * 保留 `A-Z a-z 0-9 - _ . ! ~ * ' ( )`，其余字节一律 `%XX`（大写十六进制）；空格是 `%20`
 * （不是 form 编码的 `+`）。这套字符集与平台的 `encodeURIComponent` 相同。
 *
 * 先把落单的代理项换成 U+FFFD 再编码，故本函数不抛 `URIError`（见 `well-formed.ts`）。
 */
export function percentEncode(value: string): string {
  return encodeURIComponent(wellFormed(value));
}
