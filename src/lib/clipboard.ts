/**
 * 把一段文本写进剪贴板。
 *
 * 两条通道都得有：桌面 WebView2 里异步剪贴板权限可能被拒，`serve-web` 跑在非安全上下文
 * （`http://<局域网IP>:4173`）时 `navigator.clipboard` 根本不存在 —— 只试异步接口的话，
 * 界面在两种部署形态下都只能报「复制失败」。故先试异步接口，失败或不存在时回退到隐藏
 * textarea + `execCommand('copy')`。永不抛，失败返回 `false`。
 *
 * `doc` 只为单测注入：本仓的测试跑在 Node 里，没有 DOM。
 */
export async function copyText(text: string, doc: Document = document): Promise<boolean> {
  // 取宿主窗口的 navigator：注入假 Document 时测试不必去改 Node 的全局对象。
  const nav =
    doc.defaultView?.navigator ?? (typeof navigator === 'undefined' ? undefined : navigator);
  try {
    if (nav?.clipboard?.writeText) {
      await nav.clipboard.writeText(text);
      return true;
    }
  } catch {
    // 权限被拒或非安全上下文：落到下面的同步回退，不把失败直接交给调用方。
  }
  return copyByExecCommand(text, doc);
}

/** 同步回退。`execCommand` 返回它自己是否成功，故失败也是这条路径的结论，不是异常。 */
function copyByExecCommand(text: string, doc: Document): boolean {
  // 键盘用户点了复制不该丢掉焦点：`select()` 会把焦点抢到那个 textarea 上。
  const previous = doc.activeElement as HTMLElement | null;
  let textarea: HTMLTextAreaElement | null = null;
  try {
    textarea = doc.createElement('textarea');
    textarea.value = text;
    // 只读避免移动端弹出软键盘，也避免用户的可访问性工具读到一份可编辑的副本。
    textarea.readOnly = true;
    textarea.style.position = 'fixed';
    textarea.style.top = '0';
    textarea.style.left = '0';
    textarea.style.opacity = '0';
    textarea.style.pointerEvents = 'none';
    doc.body.appendChild(textarea);
    textarea.select();
    return doc.execCommand('copy');
  } catch {
    return false;
  } finally {
    if (textarea) {
      try {
        doc.body.removeChild(textarea);
      } catch {
        // 已经不在文档里：没有可清理的。
      }
    }
    if (previous && typeof previous.focus === 'function') {
      try {
        previous.focus();
      } catch {
        // 元素在复制期间被卸载：焦点无处可还。
      }
    }
  }
}
