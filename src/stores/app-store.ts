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

import { useMemo } from 'react';
import { create } from 'zustand';
import { createBootstrapSlice } from './bootstrap-slice';
import { createCrudSlice } from './crud-slice';
import { createDirtyTabsSlice } from './dirty-tabs-slice';
import { createUiSlice } from './ui-slice';
import { createMcpSlice } from './mcp-slice';
import type { DraftEndpoint } from './store-types';
import type { EndpointDetail, ResourceNode } from '@/types/domain';

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

/**
 * The node behind the active **saved** endpoint, or `null`.
 *
 * Answers "a row in `nodes`" only. An unsaved draft endpoint is not one —
 * it exists solely in `draftEndpoints` — so this returns `null` for it by
 * design. Anything that renders an endpoint, rather than reasoning about
 * the database, wants [`useActiveEndpointResource`] instead, which
 * answers for both storage forms in one shape.
 *
 * One selector, not a joined `{ node, detail }` object. Zustand v5
 * compares selector results with `Object.is`, so building a fresh object
 * here would return a new reference on every store write and re-render
 * every consumer forever. Both halves are stable references instead, and
 * callers that want the pair join them in a `useMemo` — see
 * `useActiveEndpointResource`.
 */
export const useActiveEndpoint = () =>
  useAppStore((s) => {
    // If the active tab is an endpoint, prefer that id — it stays in sync
    // with `setActiveEndpoint` (see setActiveEndpoint below) and reflects
    // the user's actual selection even when there are stale references.
    const activeTab = s.openTabs.find((t) => t.id === s.activeTabId);
    const id = activeTab?.kind === 'endpoint' ? activeTab.id : s.activeEndpointId;
    if (!id) return null;
    const node = s.nodes.find((n) => n.id === id);
    return node && node.kind === 'endpoint' ? node : null;
  });

/** The active endpoint's request configuration, or `undefined`. */
export const useActiveEndpointDetail = () =>
  useAppStore((s) => {
    const activeTab = s.openTabs.find((t) => t.id === s.activeTabId);
    const id = activeTab?.kind === 'endpoint' ? activeTab.id : s.activeEndpointId;
    return id ? s.endpointDetails[id] : undefined;
  });

// ---------------------------------------------------------------------------
//  Endpoint resource — saved row or in-memory draft, one shape
// ---------------------------------------------------------------------------

/**
 * The endpoint the active tab shows, whether it has been saved yet.
 *
 * WHY THIS IS NOT JUST THE TWO SELECTORS ABOVE
 * --------------------------------------------
 * `createEndpointAt` no longer writes anything: a new endpoint is a
 * `DraftEndpoint` in memory until Save persists it (see
 * `slices/ui-draft-endpoints.ts`). A draft has no `nodes` entry and no
 * `endpoint_details` row, so both selectors above answer "nothing" for
 * one — and every consumer would then have to re-answer "is there
 * something here, and is it saved?" for itself.
 *
 * Synthesising the two missing halves puts a draft in front of consumers
 * as the *same shape* as a saved endpoint. That is not a convenience
 * shortcut: it is what lets the HTTP tester run one code path, so an
 * unsaved endpoint is debuggable with the same editors, the same
 * validation and the same cURL preview as a saved one — which is the
 * entire reason to allow sending before saving.
 *
 * Note what is deliberately *not* synthesised: `params` / `headers` /
 * `body`. Those live in `endpointDrafts[draftId]`, keyed by the same id,
 * and are patch-semantics against the detail this function returns. The
 * tester's existing overlay (`draft?.params ?? detail?.params ?? []`)
 * therefore picks them up untouched, and a draft nobody has edited
 * correctly reports none.
 *
 * The allocation is in `useMemo`, never in a selector — see the hard
 * constraint at the top of this file.
 */
export function useActiveEndpointResource():
  | { id: string; isDraft: true; node: ResourceNode; detail: EndpointDetail }
  | { id: string; isDraft: false; node: ResourceNode; detail: EndpointDetail }
  | null {
  const tabKind = useAppStore((s) => s.openTabs.find((t) => t.id === s.activeTabId)?.kind);
  const savedNode = useAppStore((s) =>
    s.activeTabId
      ? (s.nodes.find((n) => n.id === s.activeTabId && n.kind === 'endpoint') ?? null)
      : null,
  );
  const savedDetail = useAppStore((s) =>
    s.activeTabId ? s.endpointDetails[s.activeTabId] : undefined,
  );
  const draft = useAppStore((s) => (s.activeTabId ? s.draftEndpoints[s.activeTabId] : undefined));

  return useMemo(() => {
    if (tabKind === 'endpoint-draft') {
      // A draft tab whose document is gone (discarded, or pruned out from
      // under us) has nothing to show. `null` is the honest answer and
      // the caller's empty state covers it.
      if (!draft) return null;
      return { id: draft.id, isDraft: true, node: draftNode(draft), detail: draftDetail(draft) };
    }
    if (tabKind === 'endpoint') {
      if (!savedNode || !savedDetail) return null;
      return { id: savedNode.id, isDraft: false, node: savedNode, detail: savedDetail };
    }
    return null;
  }, [tabKind, savedNode, savedDetail, draft]);
}

/**
 * A draft presented as a tree row.
 *
 * A draft is not in `nodes` and never will be until Save, so `sortOrder`
 * is a placeholder — `addNode` assigns the real one when the row is
 * finally written. `updatedAt` mirrors `createdAt` because nothing has
 * happened to this "row" since it was born.
 */
function draftNode(d: DraftEndpoint): ResourceNode {
  return {
    id: d.id,
    rootId: d.rootId,
    parentId: d.parentId,
    kind: 'endpoint',
    name: d.name,
    description: d.description,
    sortOrder: 0,
    createdAt: d.createdAt,
    updatedAt: d.createdAt,
  };
}

/**
 * A draft's document fields presented as an `EndpointDetail`.
 *
 * `params` / `headers` / `body` start empty on purpose — see the note on
 * `useActiveEndpointResource`. They are not "lost": the request-config
 * editors write them into `endpointDrafts` under this same id, and
 * `saveDraftEndpoint` merges both halves into the single row it writes.
 */
function draftDetail(d: DraftEndpoint): EndpointDetail {
  return {
    nodeId: d.id,
    method: d.method,
    url: d.url,
    environmentId: d.environmentId,
    baseUrlDefinitionId: d.baseUrlDefinitionId,
    baseUrlOverride: d.baseUrlOverride,
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: d.auth,
    docs: d.docs,
    tags: d.tags,
  };
}

// ---------- re-exports for consumer-facing types ----------
// The slices declare their types alongside their slice creators; this barrel
// re-exports the ones consumers import directly from `@/stores/app-store` so
// existing call sites (`type TabKind`, `type OpenTab`, `type NewType`, etc.)
// don't have to learn the new slice modules. Keeping the import path stable
// also makes any future slice refactor invisible to consumers.

export type { Theme, RequestSettings } from './store-types';
export type { NewDraftKind, TabKind, SettingsCategoryId } from './store-types';
export type { OpenTab, EndpointDraft, NewItemDraft, ApiResponseExample } from './store-types';
export type { DraftEndpoint } from './store-types';
export { SETTINGS_TAB_ID, DRAFT_ID_PREFIX, UNTITLED_ENDPOINT_NAME } from './store-types';
