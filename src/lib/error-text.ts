/**
 * 把一个来源不明的错误转成可显示的字符串。
 *
 * 三种取值各有来源：
 *
 * - `Error` → 它的 `message`（浏览器侧 `invokeCmd` 自己抛的就是这一类）。
 * - `{ code, message }` 这样的普通对象 → 它的 `message`。**桌面端 invoke 的失败就长这样**：
 *   Tauri 2 把实现了 `Serialize` 的 `AppError` 当作普通对象 reject，故它不是 `Error`，
 *   直接 `String(e)` 会得到一句 `[object Object]`——把唯一有信息量的那句报错丢掉。
 * - 其余 → `String(e)`。
 *
 * `String(e)` 自己也会抛：`Object.create(null)` 这样的取值没有 `toString`，转字符串
 * 时抛 `TypeError`。批量发送时这个异常发生在逐条的 catch 里，故一次就会让整批停下，
 * 这里给它一句能显示的话。
 */
export function errorText(e: unknown): string {
  if (e instanceof Error) {
    return e.message;
  }
  // 后端的错误在线上就是 `{ code, message }`（见 `crates/simple-post-core/src/error.rs`
  // 里手写的 `Serialize`）。`message` 才是给人看的那一句，`code` 是给程序判断的。
  if (typeof e === 'object' && e !== null && 'message' in e) {
    const message = e.message;
    if (typeof message === 'string' && message !== '') {
      return message;
    }
  }
  try {
    return String(e);
  } catch {
    return '未知错误';
  }
}
