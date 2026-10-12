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
import type { AuthConfig, HttpMethod, KeyValue, RequestBody } from '@/types/domain';

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
 * The kinds that still open a *create dialog*. An endpoint is deliberately
 * absent: creating one asks no questions (the node you clicked is the
 * parent), so it writes straight to a frontend draft and lands in rename —
 * see `slices/ui-new-item.ts::createEndpointAt`.
 *
 * NOTE: `ProjectTree` mirrors this shape for its own hook bindings; the two
 * are intentionally the same so the tree's menu items can be typed against
 * the store's action signature.
 */
export type NewDraftKind = 'project' | 'module' | 'collection';

/**
 * Kinds of tabs the right-pane workspace can show.
 *
 * `endpoint-draft` is the one kind whose id is **not** a `nodes` row: it is
 * a `DraftEndpoint` id, minted in the renderer and only persisted when the
 * user presses Save (see `slices/ui-draft-endpoints.ts`). It is a distinct
 * kind rather than an `endpoint` with a missing row on purpose — every
 * "does this entity still exist?" sweep (`pruneStaleTabs`, the cascade
 * deletes, the dirty-close queue) has to be able to tell "deleted" from
 * "not written yet", and folding the two would close a brand-new draft tab
 * the instant the tree refreshed.
 */
export type TabKind =
  'project' | 'module' | 'collection' | 'endpoint' | 'endpoint-draft' | 'settings';

/**
 * Id prefix for anything that lives only in memory. A real node id is a
 * bare `uid()`, so `draft:` cannot collide with one — which is what lets a
 * draft tab, a draft endpoint and a real endpoint share the one `openTabs`
 * array without a lookup that has to guess which it is holding.
 */
export const DRAFT_ID_PREFIX = 'draft:';

/**
 * An endpoint that exists only in the renderer.
 *
 * WHOLE-DOCUMENT semantics, unlike `EndpointDraft` (which is a patch over
 * an already-stored `EndpointDetail`). There is no stored row for a draft
 * to be a patch *against*, so every field the save will write is present
 * here, starting from the defaults `blankDetail` would have supplied.
 *
 * The split is the whole point of the design: params / headers / body are
 * *not* fields, because they arrive through the request-config editors,
 * which write to `endpointDrafts` like they do for a saved endpoint.
 * `saveDraftEndpoint` merges the two halves — see `slices/ui-draft-endpoints.ts`.
 */
export interface DraftEndpoint {
  /** Locally minted unique id, also used as the tab id. Always starts with `DRAFT_ID_PREFIX`. */
  id: string;
  /** Node the endpoint will be written under (a module or a collection). */
  parentId: string;
  /** Owning project. Used by the UI to scope the draft; see the save path for why it is not replayed. */
  rootId: string;
  /** Interface name — `UNTITLED_ENDPOINT_NAME` until renamed, then updated in place. */
  name: string;
  /**
   * One-line summary, like `ResourceNode.description`.
   *
   * A node field rather than a detail field, so it is written through
   * `addNode`'s own `description` and *not* through the merged `detail` —
   * but it lives here rather than nowhere, because the workspace's
   * description editor is enabled for a draft exactly as it is for a
   * saved endpoint, and an editor that silently discards its input is
   * worse than one that is absent.
   */
  description: string;
  method: HttpMethod;
  /** URL path without any baseUrl, same meaning as `EndpointDetail.url`. */
  url: string;
  auth: AuthConfig;
  environmentId: string | null;
  baseUrlDefinitionId: string | null;
  baseUrlOverride: string | null;
  tags: string[];
  docs: string;
  createdAt: number;
}

/**
 * The name a draft endpoint is born with.
 *
 * Re-exported rather than redeclared: `ui-new-item.ts` has owned it since
 * before drafts existed, and two constants with the same value would let
 * the dialog path and the draft path drift apart silently. Sub-factories
 * import it from here — they must not import each other.
 */
export { UNTITLED_ENDPOINT_NAME } from './slices/ui-new-item';

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
 * One project's own tab strip, set aside while the user is somewhere else.
 *
 * `openTabs` is a single flat array because the strip renders one list at
 * a time — but that list is per-project. An endpoint tab belonging to
 * project A means nothing while project B is open, and `pruneStaleTabs`
 * would eventually retire it as "no longer in the tree". So switching
 * projects stashes the outgoing set here and restores the incoming one.
 *
 * ONLY THE TAB LIST IS KEPT, and that is the point. Everything an unsaved
 * tab holds — `draftEndpoints[draftId]`, `endpointDrafts[id]`, its entry
 * in `dirtyTabIds` — is keyed by tab id and is deliberately never touched
 * by a switch. "Leave project A with three dirty tabs and a named draft,
 * come back, find them exactly as they were" therefore needs no second
 * copy of any of it: this record says *which* tabs to show, and the maps
 * they were always keyed under still hold their contents.
 */
export interface ProjectTabSet {
  tabs: OpenTab[];
  /** Which of them was in front, or `null` if the user closed it last. */
  activeTabId: string | null;
}

/**
 * Per-endpoint unsaved edits. Lives in `endpointDrafts` (not in
 * HttpTester local state) so that switching tabs and switching back
 * preserves the user's pending edits. Every field is individually
 * optional — only the fields the user actually edited are present, and
 * an absent field means "same as the stored `EndpointDetail`".
 *
 * This used to cover just `url` / `method`, which made the dirty flag
 * mean "you typed in the URL bar" and nothing else. Every request-config
 * editor (params / headers / body / auth / docs / tags) writes here too,
 * so one rule covers the whole tab: **edits live in the draft, Save
 * persists them, and the tab is dirty until then.**
 *
 * Deliberately *not* here: `baseUrlDefinitionId` / `baseUrlOverride`.
 * Those are connection configuration, committed the moment the picker
 * row is clicked — the same "no Save button" behaviour they have always
 * had.
 */
export interface EndpointDraft {
  url?: string;
  method?: HttpMethod;
  params?: KeyValue[];
  headers?: KeyValue[];
  body?: RequestBody;
  auth?: AuthConfig;
  docs?: string;
  tags?: string[];
}

/** In-memory draft of one of the create dialogs. */
export interface NewItemDraft {
  open: boolean;
  type: NewDraftKind | null;
  parentModuleId: string;
  parentCollectionId: string;
  name: string;
  description: string;
  color: string;
}

/** Saved example — same shape as ApiResponse but with a stable `id` for delete. */
export type ApiResponseExample = import('@/types/domain').ApiResponse & { id: string };
