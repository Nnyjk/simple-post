/**
 * 桌面端自动更新：检查新版、读发布说明、下载安装、重启。
 *
 * 只有 Tauri 外壳里这几步成立：`serve-web` 形态没有 updater 插件，也没有可重启的进程。
 * 两个插件用动态 `import()`，与 `transfer-file-save.ts` 同一理由——静态引入会把只在
 * 桌面壳里能跑的包拖进 `serve-web` 的 bundle。
 */
import { IS_TAURI_SHELL } from './tauri-commands';
import { errorText } from './error-text';

export type DesktopUpdate = {
  /** 新版本号，如 `0.1.35`。 */
  version: string;
  /** 当前运行的版本号，取自 `@tauri-apps/api/app` 的 `getVersion()`。 */
  currentVersion: string;
  /** `latest.json` 的 `notes`；发布方没写时为 `null`。 */
  notes: string | null;
  /** `latest.json` 的 `pub_date`，RFC 3339；缺失时为 `null`。 */
  date: string | null;
};

/**
 * `tauri dev` 下 Vite 令 `import.meta.env.DEV` 为真。开发构建没有签名产物，
 * 更新检查在那里只会失败，故不暴露入口。
 */
export function isUpdaterAvailable(): boolean {
  return IS_TAURI_SHELL && !import.meta.env?.DEV;
}

/**
 * 检查更新。非桌面形态、没有新版、网络或签名校验出错，三种情况都返回 `null`：
 * 界面只有「有更新」与「没有更新」两种呈现，错误细节在这里没有出口。
 */
export async function checkDesktopUpdate(): Promise<DesktopUpdate | null> {
  if (!isUpdaterAvailable()) return null;

  try {
    const [{ check }, { getVersion }] = await Promise.all([
      import('@tauri-apps/plugin-updater'),
      import('@tauri-apps/api/app'),
    ]);
    const update = await check();
    if (!update) return null;

    const result: DesktopUpdate = {
      version: update.version,
      currentVersion: await getVersion(),
      notes: update.body ?? null,
      date: update.date ?? null,
    };
    // `Update` 在 Rust 侧持有一个资源；这里只取元数据，不下载，故把它还回去。
    await update.close();
    return result;
  } catch {
    return null;
  }
}

/**
 * 下载并安装更新，成功后重启进程。
 *
 * `onProgress` 的百分比在 `contentLength` 为 0（服务端没给长度，或分块传输）时给 `null`，
 * 界面据此显示不确定进度。下载结束、安装开始时阶段切到 `'install'`。
 *
 * 失败抛 `Error`：插件经 IPC 抛出的可能不是 `Error` 实例，统一包一层再交给界面。
 */
export async function installDesktopUpdate(
  onProgress: (percent: number | null, phase: 'download' | 'install') => void,
): Promise<void> {
  if (!isUpdaterAvailable()) {
    throw new Error('当前形态不支持自动更新。');
  }

  const { check } = await import('@tauri-apps/plugin-updater');
  const update = await check();
  if (!update) {
    throw new Error('没有可安装的更新。');
  }

  let contentLength = 0;
  let received = 0;
  try {
    await update.downloadAndInstall((event) => {
      if (event.event === 'Started') {
        contentLength = event.data.contentLength ?? 0;
        received = 0;
        onProgress(contentLength > 0 ? 0 : null, 'download');
        return;
      }
      if (event.event === 'Progress') {
        received += event.data.chunkLength;
        const percent =
          contentLength > 0 ? Math.min(100, Math.round((received / contentLength) * 100)) : null;
        onProgress(percent, 'download');
        return;
      }
      // `Finished` 是下载结束、安装开始的时刻。
      onProgress(null, 'install');
    });
  } catch (err) {
    throw err instanceof Error ? err : new Error(errorText(err));
  }

  // Windows 的 NSIS 安装器启动后会接管进程；macOS / Linux 靠这一步跑新版本。
  const { relaunch } = await import('@tauri-apps/plugin-process');
  await relaunch();
}
