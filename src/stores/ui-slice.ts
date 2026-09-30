/**
 * UI slice — owns every piece of state that the renderer manipulates
 * but the Tauri daemon doesn't persist.
 *
 * LAYOUT
 * ------
 * This file is the slice's public surface only. The implementations live
 * in one sub-factory per concern, under `./slices/`, mirroring how
 * `crud-slice.ts` is organised:
 *
 *   slices/ui-tabs.ts       openTabs / activeTabId / dirtyTabIds /
 *                           endpointDrafts / activeSettingsSection
 *   slices/ui-response.ts   lastResponse / requestPending /
 *                           responseHistory / responseExamples
 *   slices/ui-prefs.ts      theme / requestSettings (localStorage)
 *   slices/ui-new-item.ts   newItem draft + submitNewItem
 *
 * The sub-factories are composed HERE, not registered as separate slices
 * in `app-store.ts`, and that is the whole point of the split. Several
 * actions deliberately write `set()` keys owned by other slices in the
 * same atomic write — `submitNewItem` writes `lastCreatedId` and
 * reaches for `addProject` / `addNode` / `setActiveEndpoint` in one
 * flow, and `deleteNode` (crud-slice) prunes `responseHistory`,
 * `responseExamples`, `openTabs` and `endpointDrafts` together so an
 * IPC failure restores all of them as one state. Rewriting any of those
 * as a sequence of `get().closeTab()`-style calls would turn one `set()`
 * into several and change what a failure restores. So they stay whole,
 * and the files are grouped by concern instead of by slice.
 *
 * The topology is a star: sub-factories may import `../store-types` and
 * the shared `AppState` type, and they never import each other or this
 * file. Each takes the standard `(set, get, store)` Zustand triple and
 * returns only its own state + action object.
 */
import type { StateCreator } from 'zustand';
import type { AppState } from './app-store';
import { createUiTabs } from './slices/ui-tabs';
import type { UiTabs } from './slices/ui-tabs';
import { createUiResponse } from './slices/ui-response';
import type { UiResponse } from './slices/ui-response';
import { createUiPrefs } from './slices/ui-prefs';
import type { UiPrefs } from './slices/ui-prefs';
import { createUiNewItem } from './slices/ui-new-item';
import type { UiNewItem } from './slices/ui-new-item';

/**
 * The composed UI slice. Every member is declared next to its
 * implementation in `./slices/`; this is the single place that names
 * them all. Consumers import `useAppStore` and never see these four
 * modules directly.
 */
export interface UiSlice extends UiTabs, UiResponse, UiPrefs, UiNewItem {}

export const createUiSlice: StateCreator<AppState, [], [], UiSlice> = (set, get, store) => ({
  ...createUiTabs(set, get, store),
  ...createUiResponse(set, get, store),
  ...createUiPrefs(set, get, store),
  ...createUiNewItem(set, get, store),
});
