import type { StateCreator } from 'zustand';
import type { AppState } from '../app-store';
import type { RequestSettings, Theme } from '../store-types';

/**
 * ui-prefs — 两个能跨刷新生效的偏好。
 *
 * localStorage 助手保持模块私有：store 之外不该写这个 key。
 * 两个 setter 都整份覆盖而不是与磁盘上的内容合并，避免两个 action
 * 交错写出半条记录。
 */

const PREFS_KEY = 'simple-post:prefs';

interface PersistedPrefs {
  theme?: Theme;
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
const initialRequestSettings: RequestSettings = {
  timeoutMs: initialPrefs.requestSettings?.timeoutMs ?? 30000,
  followRedirects: initialPrefs.requestSettings?.followRedirects ?? true,
  maxResponseSizeKb: initialPrefs.requestSettings?.maxResponseSizeKb ?? 5120,
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

export const createUiPrefs: StateCreator<AppState, [], [], UiPrefs> = (set, get) => ({
  theme: initialTheme,
  requestSettings: initialRequestSettings,

  setTheme: (t) => {
    set({ theme: t });
    const s = get();
    savePrefs({ theme: t, requestSettings: s.requestSettings });
  },
  updateRequestSettings: (patch) => {
    set((s) => ({ requestSettings: { ...s.requestSettings, ...patch } }));
    const s = get();
    savePrefs({ theme: s.theme, requestSettings: s.requestSettings });
  },
});
