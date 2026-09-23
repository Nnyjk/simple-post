/**
 * Simple Post — global app store (Zustand barrel).
 *
 * --------------------------------------------------------------------------
 * SELECTOR RULES (HARD CONSTRAINT — read before adding new selectors)
 * --------------------------------------------------------------------------
 * Zustand v5 uses `Object.is` to compare selector outputs. Any selector that
 * returns a freshly-allocated value (`.map`, `.filter`, `.sort`, spread, etc.)
 * WILL trigger an infinite re-render loop in the consuming component.
 *
 * Therefore:
 *   ✅ ALLOWED:  return raw state references or single `.find()` results
 *                e.g. `s => s.projects`, `s => s.endpoints.find(e => e.id === id)`
 *   ❌ FORBIDDEN: `s => s.endpoints.filter(...)`, `s => [...s.endpoints]`,
 *                 `s => s.endpoints.sort(...)`, anything that allocates.
 *
 * All filtering / sorting / grouping that needs derived arrays MUST happen
 * inside the consuming component via `useMemo`, not inside the selector.
 *
 * The three exported `useActive*` selectors below are the canonical pattern.
 * If you need a new selector, follow the same shape (`s => s.x` or
 * `s => s.x.find(...)`). Do not introduce array-derived selectors.
 * --------------------------------------------------------------------------
 *
 * SLICING
 * --------------------------------------------------------------------------
 * The store is composed of five slice creators, each owning a tightly-scoped
 * piece of state. The slice creators are pure factories that take the
 * standard `(set, get, store)` Zustand triple and return a partial state +
 * actions object; merging the slices into one backing store is what
 * `create<AppState>((set, get, store) => ({ ...createXxxSlice(set, get, store) }))`
 * below does.
 *
 * Each slice:
 *   - Owns the state it declares and the actions that mutate that state.
 *   - Couples to the rest of the store ONLY via the shared `AppState`
 *     type. It never imports another slice module directly.
 *   - Reads cross-slice state through `get()` and writes via `set()`.
 *
 * Adding new functionality? Pick the slice that best fits (see the
 * header comment of each slice file) and extend it. If you're adding
 * a totally new concern, create a new slice file and append its
 * creator here.
 *
 * Slice inventory:
 *   - `./bootstrap-slice.ts`   — initial-load data + active selection
 *   - `./crud-slice.ts`        — all Tauri-mediated CRUD writes
 *   - `./dirty-tabs-slice.ts`  — dirty-guarded close queue
 *   - `./ui-slice.ts`          — workspace tabs + prefs + new-item modal
 *   - `./mcp-slice.ts`         — MCP daemon polling
 * --------------------------------------------------------------------------
 */

import { create } from 'zustand';
import { createBootstrapSlice } from './bootstrap-slice';
import { createCrudSlice } from './crud-slice';
import { createDirtyTabsSlice } from './dirty-tabs-slice';
import { createUiSlice } from './ui-slice';
import { createMcpSlice } from './mcp-slice';

// Compose the AppState type from the intersection of all slices. The slice
// creators export both their `*Slice` types and the `create*` function;
// using the `*Slice` types here (rather than `ReturnType<typeof create*>`)
// gives the IDE better hover info and lets us drop unused imports cleanly.
import type { BootstrapSlice } from './bootstrap-slice';
import type { CrudSlice } from './crud-slice';
import type { DirtyTabsSlice } from './dirty-tabs-slice';
import type { UiSlice } from './ui-slice';
import type { McpSlice } from './mcp-slice';

/**
 * The composed AppState — intersection of all five slices. Consumers can
 * either use the named selector hooks below (`useActiveProject`, etc.) or
 * pick a raw field via `useAppStore((s) => s.projects)`.
 */
export type AppState = BootstrapSlice & CrudSlice & DirtyTabsSlice & UiSlice & McpSlice;

/**
 * The composed Zustand store. Slice creators are called with the same
 * `(set, get, store)` triple so they all read from and write to the same
 * backing store instance — that's how the cross-slice actions work without
 * any module-level coupling.
 */
export const useAppStore = create<AppState>()((set, get, store) => ({
  ...createBootstrapSlice(set, get, store),
  ...createCrudSlice(set, get, store),
  ...createDirtyTabsSlice(set, get, store),
  ...createUiSlice(set, get, store),
  ...createMcpSlice(set, get, store),
}));

// ---------- selectors ----------
// IMPORTANT: every selector below returns a raw state reference (or a single
// `.find()` result). Do not add selectors that allocate new arrays/objects —
// that will trigger infinite re-renders under Zustand v5.

/**
 * Currently active project, or `undefined` if `bootstrap()` hasn't run yet
 * (e.g. the very first render before the daemon data arrives). Callers
 * must tolerate `undefined` and render a loading state.
 */
export const useActiveProject = () =>
  useAppStore((s) => s.projects.find((p) => p.id === s.activeProjectId));

/**
 * The currently active workspace tab (raw reference), or `null` when no
 * tab is open. Returned as the `OpenTab` object so consumers can switch
 * on `kind` to pick the right renderer.
 */
export const useActiveTab = () =>
  useAppStore((s) => s.openTabs.find((t) => t.id === s.activeTabId) ?? null);

export const useActiveEnvironment = () =>
  useAppStore((s) => s.environments.find((e) => e.id === s.activeEnvironmentId));

export const useActiveEndpoint = () =>
  useAppStore((s) => {
    // If the active tab is an endpoint, prefer that id — it stays in sync
    // with `setActiveEndpoint` (see setActiveEndpoint below) and reflects
    // the user's actual selection even when there are stale references.
    const activeTab = s.openTabs.find((t) => t.id === s.activeTabId);
    const id = activeTab?.kind === 'endpoint' ? activeTab.id : s.activeEndpointId;
    return s.endpoints.find((e) => e.id === id) ?? null;
  });

// ---------- re-exports for consumer-facing types ----------
// The slices declare their types alongside their slice creators; this barrel
// re-exports the ones consumers import directly from `@/stores/app-store` so
// existing call sites (`type TabKind`, `type OpenTab`, `type NewType`, etc.)
// don't have to learn the new slice modules. Keeping the import path stable
// also makes any future slice refactor invisible to consumers.

export type { Theme, RequestSettings } from './store-types';
export type { NewType, TabKind, SettingsCategoryId } from './store-types';
export type { OpenTab, EndpointDraft, NewItemDraft, ApiResponseExample } from './store-types';
export { SETTINGS_TAB_ID } from './store-types';
