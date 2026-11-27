/**
 * 选一个文件，拿回「请求该怎么引用它」的那份描述。
 *
 * 两种形态拿到的不是同一样东西：
 *
 * - **桌面（Tauri）**：`dialog` 插件弹原生选择框，返回绝对路径。执行层（同一个进程）在发
 *   请求时按路径现读文件，故这里只带路径与文件名，字节不进渲染进程。
 * - **Web / Docker**：浏览器的 `<input type=file>` 只给 `File` 句柄，没有服务端能读的路径。
 *   此时的描述是「文件名 + 字节数」，字节本体由 `web-uploads.ts` 暂存，发送时随
 *   `multipart/form-data` 一起送上去（服务端边收边转发，不落盘）。
 *
 * 动态 import 只在桌面分支做：浏览器形态里没有 `dialog` 插件，静态引入会让 `serve-web` 的
 * bundle 白背一份 Tauri 代码。
 */
import { IS_TAURI_SHELL } from './tauri-commands';

/** 选中一个文件。用户取消时给 `null`。 */
export interface PickedFile {
  /**
   * 桌面：绝对路径，执行层按它读字节。
   * 浏览器：空串——没有路径可给，字节走 `file`。
   */
  path: string;
  /** 文件名（路径的最后一段），界面上显示它，也是线上那份 `filename`。 */
  name: string;
  /** 字节数。浏览器形态靠它声明上传长度，服务端据它写 `Content-Length`。 */
  size?: number;
  /** 字节本体。只有浏览器形态有；调用方交给 `rememberUpload` 暂存。 */
  file?: File;
}

/** 路径的最后一段。Windows 上是 `\` 分隔，故两种都认。 */
export function baseName(path: string): string {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return cut >= 0 ? path.slice(cut + 1) : path;
}

/**
 * 浏览器形态的选择框：`input.click()` 打开原生对话框，选中后 `change` 带回 `File`。
 *
 * 取消没有回调：`cancel` 事件在现代浏览器里有，没有时这个 Promise 就悬着（不占资源，也不
 * 会写错任何状态）。故取消路径只认 `cancel`。
 */
function pickWithInput(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    // 不能 `display:none`：iOS Safari 会拒绝打开隐藏 input 的选择框。
    input.style.position = 'fixed';
    input.style.left = '-9999px';
    const done = (file: File | null): void => {
      input.remove();
      resolve(file);
    };
    input.addEventListener('change', () => done(input.files?.[0] ?? null));
    input.addEventListener('cancel', () => done(null));
    document.body.appendChild(input);
    input.click();
  });
}

/**
 * 弹选择框，返回文件描述。取消时 `null`。
 *
 * 失败（原生对话框起不来之类）会抛：那才是调用方要显示给用户的错误。
 */
export async function pickLocalFile(): Promise<PickedFile | null> {
  if (!IS_TAURI_SHELL) {
    const file = await pickWithInput();
    if (!file) return null;
    return { path: '', name: file.name, size: file.size, file };
  }

  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({ multiple: false, directory: false });
  if (typeof picked !== 'string') return null;
  return { path: picked, name: baseName(picked) };
}
