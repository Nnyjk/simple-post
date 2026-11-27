/**
 * 选一个本机文件，拿回它的路径。
 *
 * **存的是路径，不是字节**：执行层在发请求时按路径现读文件（桌面是同一个进程，`serve-web`
 * 是同一台机器）。故这里只需要路径与一点显示用的元数据，不必把文件读进渲染进程。
 *
 * - **桌面（Tauri）**：`dialog` 插件弹原生选择框，返回绝对路径。动态 import，因为浏览器形态
 *   里没有这个插件，静态引入会让 `serve-web` 的 bundle 白背一份 Tauri 代码。
 * - **Web / Docker**：浏览器的 `<input type=file>` 只给 `File`，拿不到服务端能读的路径。
 *   与其发一个读不出文件的路径，不如明确说不支持——由调用方把这句话显示出来。
 */
import { IS_TAURI_SHELL } from './tauri-commands';

/** 选中一个文件。用户取消、或当前形态拿不到路径时给 `null`。 */
export interface PickedFile {
  /** 绝对路径。执行层按它读字节。 */
  path: string;
  /** 文件名（路径的最后一段），界面上显示它。 */
  name: string;
}

/**
 * 浏览器形态拿不到路径时给用户的说法。
 *
 * 说清楚**为什么**：不是没做，而是浏览器不给文件名之外的东西；用户据此知道该换桌面形态，
 * 而不是反复点同一个按钮。
 */
export const WEB_PICK_UNSUPPORTED = '浏览器形态拿不到文件的本地路径，请用桌面版选择文件';

/** 路径的最后一段。Windows 上是 `\` 分隔，故两种都认。 */
export function baseName(path: string): string {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return cut >= 0 ? path.slice(cut + 1) : path;
}

/**
 * 弹选择框，返回路径与文件名。取消时 `null`；浏览器形态下抛
 * {@link WEB_PICK_UNSUPPORTED}——那是调用方要显示给用户的说法，不是意外故障。
 */
export async function pickLocalFile(): Promise<PickedFile | null> {
  if (!IS_TAURI_SHELL) throw new Error(WEB_PICK_UNSUPPORTED);

  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({ multiple: false, directory: false });
  if (typeof picked !== 'string') return null;
  return { path: picked, name: baseName(picked) };
}
