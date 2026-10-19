/**
 * 把导出的文档交给用户。
 *
 * 两种形态、两条路径：
 * - **桌面（Tauri）**：`dialog` 插件弹原生「另存为」，`fs` 插件按用户选定的路径写盘。
 *   走这条是因为 WebView 里的 blob 下载不跨形态一致，而「另存为」是用户预期的形态。
 * - **Web / Docker**：同一个 DOM 上的 blob + `<a download>`，浏览器原生行为。
 *
 * 两个插件都是**动态 import**，不是洁癖：它们在浏览器形态里没有实现，静态引入会让
 * `serve-web` 的 bundle 白背一份 Tauri 代码，并在运行时炸掉。`IS_TAURI_SHELL` 已在
 * `tauri-commands.ts` 导出。
 */
import { IS_TAURI_SHELL } from './tauri-commands';
import type { ExportDocument } from './interchange-types';

export type SaveOutcome = 'saved' | 'cancelled';

/** 保存对话框的过滤器。`mediaType` 决定扩展名，`fileName` 兜住用户改名的情形。 */
function filtersFor(doc: ExportDocument) {
  const ext = doc.mediaType === 'application/yaml' ? 'yaml' : 'json';
  return [{ name: ext.toUpperCase(), extensions: [ext] }];
}

/**
 * `fs` 的 scope 只放行了用户 profile 下的路径（Documents / Downloads / Desktop 都在
 * 里面），存到别处会被插件门禁拒绝。那条错误长得像 IO 故障，用户会以为是磁盘满了，
 * 故这里换成人话。
 */
export function humanizeSaveError(err: unknown): Error {
  const raw = err instanceof Error ? err.message : String(err);
  if (/not allowed|scope|permission|forbidden/i.test(raw)) {
    return new Error('这个位置不允许写入。请存到「文档」「下载」或桌面目录下。');
  }
  return err instanceof Error ? err : new Error(raw);
}

export async function saveGeneratedDocument(doc: ExportDocument): Promise<SaveOutcome> {
  if (!IS_TAURI_SHELL) {
    downloadInBrowser(doc);
    return 'saved';
  }

  const { save } = await import('@tauri-apps/plugin-dialog');
  const path = await save({
    defaultPath: doc.fileName,
    filters: filtersFor(doc),
  });
  if (!path) return 'cancelled';

  const { writeTextFile } = await import('@tauri-apps/plugin-fs');
  try {
    await writeTextFile(path, doc.content);
  } catch (err) {
    throw humanizeSaveError(err);
  }
  return 'saved';
}

/** 复制到剪贴板。次要出口：原生保存对话框被取消时，用户仍然拿得到内容。 */
export async function copyGeneratedDocument(doc: ExportDocument): Promise<void> {
  await navigator.clipboard.writeText(doc.content);
}

/**
 * 浏览器形态的下载。`Blob` 的 MIME 用 `application/json` / `application/yaml`，
 * 于是「打开方式」列表里会落到正确的程序上。
 */
function downloadInBrowser(doc: ExportDocument): void {
  const blob = new Blob([doc.content], { type: `${doc.mediaType};charset=utf-8` });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = doc.fileName;
  document.body.appendChild(a);
  a.click();
  a.remove();
  // 不撤销的话 blob 会一直挂在内存上直到页面卸载——导出几份大文档就够了。
  URL.revokeObjectURL(url);
}
