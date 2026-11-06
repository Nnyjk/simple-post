/**
 * 把一个来源不明的错误转成可显示的字符串。
 *
 * `Error` 取它的 `message`，其余取值走 `String(e)`。
 *
 * `String(e)` 自己也会抛：`Object.create(null)` 这样的取值没有 `toString`，转字符串
 * 时抛 `TypeError`。批量发送时这个异常发生在逐条的 catch 里，故一次就会让整批停下，
 * 这里给它一句能显示的话。
 */
export function errorText(e: unknown): string {
  if (e instanceof Error) {
    return e.message;
  }
  try {
    return String(e);
  } catch {
    return '未知错误';
  }
}
