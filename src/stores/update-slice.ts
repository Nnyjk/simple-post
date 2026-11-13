/**
 * 更新切片 —— 检查、部署形态、执行安装与「已忽略」。
 *
 * 三件事刻意放在同一片里：它们共享 `updateBusy` 与 `updateError`，且界面上的每个入口
 * （顶栏药丸、更新对话框、设置卡片）读的都是同一份结果，分成几片必然各读一半。
 *
 * **两种形态走两条路，不共用一次检查**：`update_check` / `update_deployment` /
 * `update_apply` 只有 `serve-web` 注册（桌面壳没有对应的 Tauri 命令），桌面端由 updater
 * 插件自己去读签名清单、自己装，故桌面分支一次也不碰那三条命令——碰了只会拿到
 * 「command not found」。
 */
import type { StateCreator } from 'zustand';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { checkDesktopUpdate, installDesktopUpdate, isUpdaterAvailable } from '@/lib/updater';
import type { UpdateApplyResult, UpdateCheck, UpdateDeployment } from '@/types/update';
import { errorText } from '@/lib/error-text';
import { describeCheckError } from '@/lib/update/messages';
import { dueForCheck } from '@/lib/update/time';
import type { AppState } from './app-store';
import {
  readUpdatePrefs,
  saveDismissedUpdateVersion,
  saveLastUpdateCheckAt,
} from './slices/ui-prefs';

/** 一次检查的结果，外加插件报的发布日期（只有桌面端才有）。 */
interface CheckOutcome {
  info: UpdateCheck;
  releaseDate: string | null;
}

/**
 * 桌面端读当前版本号。`update_check` 只有 web 有，而设置卡片在「没有新版」时也得写得出
 * 当前版本，故这里直接问 Tauri 的 app 接口。动态 `import()`：与 updater 插件同理，
 * 静态引入会把只在桌面壳里存在的包拖进 `serve-web` 的 bundle。
 */
async function desktopVersion(): Promise<string> {
  try {
    const { getVersion } = await import('@tauri-apps/api/app');
    return await getVersion();
  } catch (e) {
    console.warn('[store] app version unavailable', e);
    return '';
  }
}

/**
 * 桌面端的一次检查。插件回答「有没有可安装的新版」；没有新版时它是 `null`，而界面还要写
 * 「已是最新」并给出当前版本，故版本号单独取一次。
 *
 * `url` 留空：桌面上没有 GitHub 接口给的那份 release 页地址，按 tag 拼出仓库路径就是猜。
 */
async function desktopCheck(): Promise<CheckOutcome> {
  const [desktop, current] = await Promise.all([checkDesktopUpdate(), desktopVersion()]);
  const checkedAt = new Date().toISOString();
  if (!desktop) {
    return {
      info: {
        current,
        latest: null,
        available: false,
        checkedAt,
        cached: false,
        notes: null,
        url: null,
        error: null,
      },
      releaseDate: null,
    };
  }
  return {
    info: {
      current: desktop.currentVersion || current,
      latest: desktop.version,
      available: true,
      checkedAt,
      cached: false,
      notes: desktop.notes,
      url: null,
      error: null,
    },
    releaseDate: desktop.date,
  };
}

export interface UpdateSliceState {
  /** 最近一次检查的结论。启动时还没查过为 `null`。 */
  updateInfo: UpdateCheck | null;
  /** 本部署的形态与更新方式。读不到时为 `null`，界面按「源码 / 二进制」的说明呈现。 */
  updateDeployment: UpdateDeployment | null;
  /** 检查、下载安装、执行更新三者任一在进行中。 */
  updateBusy: boolean;
  /** 最近一次失败的中文原因；成功或重新开始时清空。 */
  updateError: string | null;
  /** 用户点了「稍后再说」的那个版本。与 `updateInfo.latest` 相同则顶栏不出现入口。 */
  dismissedVersion: string | null;
  /** 桌面端新版的发布时间（`latest.json` 的 `pub_date`）。web 形态没有这个值。 */
  updateReleaseDate: string | null;
  /** 桌面端下载进度：0–100，`null` 表示总量未知（显示不确定态）。 */
  updateProgress: number | null;
  /** 桌面端当前阶段：下载还是安装。不在安装流程里时为 `null`。 */
  updatePhase: 'download' | 'install' | null;
  /** web + docker 的执行结果。`exitCode` 非 0 时界面要显著标红。 */
  applyResult: UpdateApplyResult | null;
}

export interface UpdateSliceActions {
  /** 查一次。`force` 绕过后端缓存；失败不抛，原因落在 `updateError`。 */
  checkForUpdate: (force?: boolean) => Promise<void>;
  /** 读本部署的更新方式（容器 / 二进制 / 能否就地执行）。读不到时保持 `null`。 */
  loadDeployment: () => Promise<void>;
  /** 执行容器内的更新命令，结果落在 `applyResult`。 */
  applyUpdate: () => Promise<void>;
  /** 把某个版本记进「已忽略」。下次检查到同一个版本时顶栏不再出现入口。 */
  dismissUpdate: (version: string) => void;
  /** 启动后的一次自动检查：24 小时节流，失败静默。 */
  autoCheckUpdate: () => Promise<void>;
  /** 桌面端「下载并安装」。成功后由 updater 模块重启进程，界面不再接管。 */
  runDesktopInstall: () => Promise<void>;
}

export type UpdateSlice = UpdateSliceState & UpdateSliceActions;

const initialPrefs = readUpdatePrefs();

export const createUpdateSlice: StateCreator<AppState, [], [], UpdateSlice> = (set, get) => ({
  updateInfo: null,
  updateDeployment: null,
  updateBusy: false,
  updateError: null,
  dismissedVersion: initialPrefs.dismissedVersion || null,
  updateReleaseDate: null,
  updateProgress: null,
  updatePhase: null,
  applyResult: null,

  checkForUpdate: async (force = false) => {
    // 已经有一次检查在跑就不再叠一次：自动检查与用户点击可能同时发生，两个回答谁先到说不准。
    if (get().updateBusy) return;
    set({ updateBusy: true, updateError: null });
    try {
      const { info, releaseDate } = isUpdaterAvailable()
        ? await desktopCheck()
        : {
            info: await invokeCmd<UpdateCheck>(COMMANDS.updateCheck, { force }),
            releaseDate: null,
          };
      set({
        updateInfo: info,
        updateReleaseDate: releaseDate,
        updateBusy: false,
        updateError: describeCheckError(info.error),
      });
      // 只在真拿到结论时记账：一次限流不该把接下来 24 小时的自动检查一起吃掉。
      if (!info.error) saveLastUpdateCheckAt(new Date().toISOString());
    } catch (e) {
      console.error('[store] update check failed', e);
      set({ updateBusy: false, updateError: errorText(e) });
    }
  },

  loadDeployment: async () => {
    // 桌面端没有部署形态这回事，问也只会拿到「command not found」。
    if (isUpdaterAvailable()) return;
    try {
      set({ updateDeployment: await invokeCmd<UpdateDeployment>(COMMANDS.updateDeployment) });
    } catch (e) {
      // 部署形态读不到不是用户要处理的事：对话框退回「按部署文档拉取新版本」的说明。
      console.warn('[store] update deployment unavailable', e);
    }
  },

  applyUpdate: async () => {
    if (get().updateBusy) return;
    set({ updateBusy: true, updateError: null, applyResult: null });
    try {
      set({ applyResult: await invokeCmd<UpdateApplyResult>(COMMANDS.updateApply) });
    } catch (e) {
      console.error('[store] update apply failed', e);
      set({ updateError: errorText(e) });
    } finally {
      set({ updateBusy: false });
    }
  },

  dismissUpdate: (version) => {
    set({ dismissedVersion: version });
    saveDismissedUpdateVersion(version);
  },

  autoCheckUpdate: async () => {
    // 节流状态在 localStorage 里：同一台机器上重启应用不该每开一次就问一遍 GitHub。
    if (!dueForCheck(readUpdatePrefs().lastCheckedAt, Date.now())) return;
    await get().checkForUpdate(false);
  },

  runDesktopInstall: async () => {
    if (get().updateBusy) return;
    set({ updateBusy: true, updateError: null, updateProgress: null, updatePhase: 'download' });
    try {
      await installDesktopUpdate((percent, phase) => {
        set({ updateProgress: percent, updatePhase: phase });
      });
    } catch (e) {
      console.error('[store] desktop update install failed', e);
      set({ updateError: errorText(e) });
    } finally {
      set({ updateBusy: false });
    }
  },
});
