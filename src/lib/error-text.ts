/**
 * 把一个来源不明的错误转成可显示的字符串。
 *
 * `Error` 取它的 `message`，其余取值走 `String(e)`。
 */
export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
