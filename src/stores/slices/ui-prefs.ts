import type { StateCreator } from 'zustand';
import type { AppState } from '../app-store';
import type { RequestSettings, Theme } from '../store-types';

/**
 * ui-prefs — the two user preferences that survive a reload.
 *
 * The localStorage helpers are private to this module (they were private
 * to `ui-slice.ts` before the split), because nothing outside the store
 * should be writing the prefs key. Both setters write the *whole* prefs
 * object rather than merging into whatever is on disk, so the two
 * actions can't interleave into a half-written record.
 */

const PREFS_KEY = 'simple-post:prefs';

/** Persisted user preferences — only the fields that survive reloads. */
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
    // localStorage may be disabled (private mode, quota); silently ignore.
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
