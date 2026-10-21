import type { StateCreator } from 'zustand';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { AppState } from '../app-store';
import type { RequestSettings, Theme } from '../store-types';

/**
 * ui-prefs —— 主题与出站请求旋钮。
 *
 * 两者的存法不同，且是有意的：
 *
 * - **主题**只在渲染层有意义，故留在 localStorage。
 * - **请求旋钮**（超时 / 跟随重定向 / 最大响应体积）决定每一次外发请求的形态，故存在
 *   `preferences` 表的 `request_settings` 行里 —— 那是后端读得到的地方。MCP 侧发请求时
 *   读的是同一行，故界面上把超时调成 5 秒，AI 客户端发出去也是 5 秒；旋钮只落在
 *   localStorage 的话，后端只知道一个写死的默认值。
 *
 * localStorage 助手保持模块私有：store 之外不该写这个 key。
 */

const PREFS_KEY = 'simple-post:prefs';

interface PersistedPrefs {
  theme?: Theme;
  /**
   * 旧版本把请求旋钮存在这里。升级后它们在 `preferences` 表里，故这个字段只用于
   * 一次性迁移（见 [`loadRequestSettings`]），迁移完成后即被删掉。
   */
  requestSettings?: Partial<RequestSettings>;
}

function loadPrefs(): PersistedPrefs {
  if (typeof window === 'undefined') return {};
  try {
    const raw = window.localStorage.getItem(PREFS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as PersistedPrefs;
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch {
    return {};
  }
}

function savePrefs(prefs: PersistedPrefs): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // localStorage 可能被禁用（隐私模式、配额）；静默忽略。
  }
}

const initialPrefs = loadPrefs();
const initialTheme: Theme = initialPrefs.theme ?? 'dark';

/**
 * 请求旋钮的默认值。**与后端 `RequestSettings::default()` 相同**（超时 30 秒、
 * 跟随重定向、最大响应 5120 KB）—— 两侧的缺省值必须是同一个数，否则「没配置过时」
 * 界面与 MCP 侧会发出不同的请求。
 */
export const REQUEST_SETTINGS_DEFAULTS: RequestSettings = {
  timeoutMs: 30_000,
  followRedirects: true,
  maxResponseSizeKb: 5_120,
};

export interface UiPrefsState {
  theme: Theme;
  requestSettings: RequestSettings;
}

export interface UiPrefsActions {
  setTheme: (t: Theme) => void;
  updateRequestSettings: (patch: Partial<RequestSettings>) => void;
}

export type UiPrefs = UiPrefsState & UiPrefsActions;

/**
 * 读一次请求旋钮，并把旧版本留在 localStorage 里的那份一次性迁进库。
 *
 * 迁移只做一次：写库成功后删掉 localStorage 里的旧字段，故它不会在之后的一次启动里
 * 盖掉用户在界面上新选的值。写库失败时保留旧值，下次启动再迁一次。
 *
 * 返回值总是完整的三个字段：后端那一行可能只存了部分字段（`RequestSettings` 的
 * 每个字段都是 `#[serde(default)]`），缺的那几项用默认值补上。
 */
export async function loadRequestSettings(): Promise<RequestSettings> {
  const legacy = loadPrefs().requestSettings;
  if (legacy) {
    const next = { ...REQUEST_SETTINGS_DEFAULTS, ...legacy };
    try {
      await invokeCmd(COMMANDS.requestSettingsUpdate, { patch: next });
      const prefs = loadPrefs();
      delete prefs.requestSettings;
      savePrefs(prefs);
    } catch (e: unknown) {
      console.error('[store] request settings migration failed', e);
    }
  }
  const stored = await invokeCmd<RequestSettings>(COMMANDS.requestSettingsGet).catch(
    (e: unknown) => {
      console.warn('[store] request settings unavailable', e);
      return REQUEST_SETTINGS_DEFAULTS;
    },
  );
  return { ...REQUEST_SETTINGS_DEFAULTS, ...stored };
}

export const createUiPrefs: StateCreator<AppState, [], [], UiPrefs> = (set, get) => ({
  theme: initialTheme,
  requestSettings: REQUEST_SETTINGS_DEFAULTS,

  setTheme: (t) => {
    set({ theme: t });
    savePrefs({ theme: t });
  },
  updateRequestSettings: (patch) => {
    const next = { ...get().requestSettings, ...patch };
    set({ requestSettings: next });
    // 写库。web 形态的 `/api/request-settings` 是**全量替换**语义，故这里发的是合并后
    // 的完整三项而不是补丁 —— 两种传输收到的都是同一份完整取值。
    void invokeCmd(COMMANDS.requestSettingsUpdate, { patch: next }).catch((e: unknown) => {
      console.error('[store] request settings update failed', e);
    });
  },
});
