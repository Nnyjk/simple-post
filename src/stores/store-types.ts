/**
 * Shared type + constant declarations used across multiple slice modules.
 *
 * Lives in its own file (rather than each slice re-declaring the same
 * types) because several slices need to agree on the same shape —
 * e.g. UiSlice's `openTabs` ↔ DirtyTabsSlice's `pendingDirtyClose`
 * both refer to `OpenTab`. Putting the canonical declaration here
 * keeps each slice's own types file tiny and avoids the "two copies
 * that drifted" failure mode.
 *
 * This file is intentionally TYPE-ONLY + a couple of small runtime
 * constants. The slice creators import from it but never call into
 * it; runtime state lives in the slice modules themselves.
 */
import type { HttpMethod } from '@/types/domain';

/** Visual theme preset. Drives CSS variables on the document root. */
export type Theme = 'dark' | 'light' | 'system';

/**
 * Tunable knobs for the HTTP executor. Persisted to localStorage so
 * users don't have to re-set them between sessions.
 */
export interface RequestSettings {
  timeoutMs: number;
  followRedirects: boolean;
  maxResponseSizeKb: number;
}

/**
 * The 4 kinds of "new" actions the global NewItemModal supports.
 *
 * NOTE: ProjectTree also has a local `NewType` alias with the same shape.
 * The two are intentionally the same so the modal can pre-fill a draft
 * for the same target as the tree's old popover.
 */
export type NewType = 'project' | 'module' | 'collection' | 'endpoint';

/** Kinds of tabs the right-pane workspace can show. */
export type TabKind = 'project' | 'module' | 'collection' | 'endpoint' | 'settings';

/**
 * Inner sections of the settings tab. Drives the left rail.
 */
export type SettingsCategoryId = 'env' | 'connection' | 'appearance' | 'request';

/**
 * Sentinel id used for the (single) settings tab. Real entities use their
 * own id, but settings is a singleton panel — using a fixed id keeps the
 * open/close/active logic identical to the entity tabs.
 */
export const SETTINGS_TAB_ID = '__settings__';

/**
 * An open tab in the right-pane workspace.
 *
 * `id` reuses the underlying entity id (project / module / collection /
 * endpoint), which makes lookup O(1) and lets `closeTab` cleanly remove
 * a tab when the user deletes the entity.
 */
export interface OpenTab {
  id: string;
  kind: TabKind;
}

/**
 * Per-endpoint unsaved URL/method draft. Lives in `endpointDrafts`
 * (not in HttpTester local state) so that switching tabs and
 * switching back preserves the user's pending edits. `url` and
 * `method` are individually optional — only the fields the user
 * actually edited are present.
 */
export interface EndpointDraft {
  url?: string;
  method?: HttpMethod;
}

/** In-memory draft of the NewItemModal. */
export interface NewItemDraft {
  open: boolean;
  type: NewType | null;
  parentModuleId: string;
  parentCollectionId: string;
  name: string;
  description: string;
  baseUrl: string;
  color: string;
}

/** Saved example — same shape as ApiResponse but with a stable `id` for delete. */
export type ApiResponseExample = import('@/types/domain').ApiResponse & { id: string };