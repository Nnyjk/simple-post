import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { Play, Save, Copy, Check, ChevronRight } from 'lucide-react';
import { useAppStore, useActiveEndpointResource, useActiveEnvironment } from '@/stores/app-store';
import { Tabs } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { InlineEdit } from '@/components/ui/InlineEdit';
import { MethodPicker } from '@/components/ui/method-picker';
import { BaseUrlPicker, type BaseUrlSelection } from '@/features/http-tester/BaseUrlPicker';
import { TagInput } from '@/components/ui/tag-input';
import { Tooltip } from '@/components/ui/tooltip';
import { type AuthConfig, type RequestBody } from '@/types/domain';
import { methodColorVar, cn } from '@/lib/utils';
import { resolveEndpointBaseUrl } from '@/lib/url';
import { validateRequest } from '@/lib/request-validation';
import { sendPreparedRequest } from '@/lib/http';
import { prepareRequest, preparedToCurl } from '@/lib/http-build';
import { draftToPatch } from '@/stores/slices/crud-helpers';
import { UNTITLED_ENDPOINT_NAME, type EndpointDraft } from '@/stores/store-types';
import { KeyValueEditor } from './KeyValueEditor';
import { buildHeaderSuggestions } from '@/lib/common-headers';
import { BodyEditor } from './BodyEditor-lazy';
import { AuthEditor } from './AuthEditor';
import { DocsEditor } from './DocsEditor-lazy';
import { EnvVarPicker, extractPartialVar, shouldShowVarPicker } from './EnvVarPicker';
import { gsap, useGSAP } from '@/lib/gsap';

const TABS: { id: TabId; label: string; disabled?: boolean }[] = [
  { id: 'params', label: 'Params' },
  { id: 'headers', label: 'Headers' },
  { id: 'body', label: 'Body' },
  { id: 'auth', label: 'Auth' },
  { id: 'docs', label: 'Docs' },
  { id: 'tests', label: 'Tests', disabled: true },
];

/**
 * Stand-ins for a missing `endpoint_details` row, so the effective-request
 * memo has something typed to read. Shared by reference on purpose: every
 * editor spreads before writing, so nothing can mutate them.
 */
const EMPTY_BODY: RequestBody = { mode: 'none', content: '' };
const EMPTY_AUTH: AuthConfig = { kind: 'none' };

type TabId = 'params' | 'headers' | 'body' | 'auth' | 'docs' | 'tests';

export function HttpTester() {
  // The endpoint is a `ResourceNode` (identity + position) and its
  // request configuration is a separate `EndpointDetail` row. Both
  // selectors return stable store references, so neither allocates.
  //
  // `useActiveEndpointResource` answers for a *draft* endpoint too: an
  // unsaved one has neither row, and the hook synthesises both from the
  // `DraftEndpoint`. Everything below this line is therefore written once
  // for "an endpoint the user is looking at", not once per storage form.
  const resource = useActiveEndpointResource();
  const endpoint = resource?.node ?? null;
  const detail = resource?.detail;
  /** True while the endpoint has never been written to the database. */
  const isDraft = resource?.isDraft ?? false;
  // Workspace-wide active env (the "ENV dev" pill in the top bar).
  // Used as the fallback env when the endpoint hasn't picked one of
  // its own — that way the baseUrl + variable resolution "just works"
  // for a fresh endpoint without forcing the user through a picker
  // first.
  const workspaceEnv = useActiveEnvironment();
  // Raw store slices — we'll derive the endpoint's project / ancestors
  // locally with useMemo. Going through `useAppStore` for each one keeps
  // the Zustand v5 "no allocation in selectors" rule intact.
  const allNodes = useAppStore((s) => s.nodes);
  const allProjects = useAppStore((s) => s.projects);
  const allEnvironments = useAppStore((s) => s.environments);
  // Pulled for the Header key autocomplete — we want the user's own
  // recently-used header names to surface in the datalist alongside
  // the built-in common list (see buildHeaderSuggestions).
  const allEndpointDetails = useAppStore((s) => s.endpointDetails);
  const updateNode = useAppStore((s) => s.updateNode);
  const updateEndpointDetail = useAppStore((s) => s.updateEndpointDetail);
  const setRequestPending = useAppStore((s) => s.setRequestPending);
  const setLastResponse = useAppStore((s) => s.setLastResponse);
  const addResponseHistory = useAppStore((s) => s.addResponseHistory);
  const setEndpointDraft = useAppStore((s) => s.setEndpointDraft);
  const clearEndpointDraft = useAppStore((s) => s.clearEndpointDraft);
  const saveDraftEndpoint = useAppStore((s) => s.saveDraftEndpoint);
  const updateDraftEndpoint = useAppStore((s) => s.updateDraftEndpoint);
  const draftSaveError = useAppStore((s) => s.draftSaveError);
  const requestPending = useAppStore((s) => s.requestPending);
  const requestSettings = useAppStore((s) => s.requestSettings);

  const [tab, setTab] = useState<TabId>('params');
  const [curlFlash, setCurlFlash] = useState(false);

  // URL/method drafts live in the store (`endpointDrafts[endpointId]`)
  // rather than in local state — that way switching tabs and switching
  // back preserves the user's pending edits. The store auto-mirrors the
  // presence of a draft into `dirtyTabIds`, so the tab strip's dirty
  // dot updates for free.
  //
  // We read the draft with a dedicated selector so HttpTester only
  // re-renders when *its own* endpoint's draft changes (a draft on
  // endpoint B while endpoint A is active should be invisible to us).
  const draft = useAppStore((s) => (endpoint ? s.endpointDrafts[endpoint.id] : undefined));
  // Name / description editing is owned by `<InlineEdit>` (keyed by
  // `endpoint.id` so the editor unmounts and resets whenever the active
  // endpoint changes — no manual "clear edit state on tab switch"
  // bookkeeping needed here).

  // EnvVarPicker state — opens when the user types `{{` in the URL input.
  // The picker is presentational: HttpTester owns the selected index and the
  // filtered entries (derived from the active environment + the partial name
  // the user has typed after the last `{{`).
  const [varPickerOpen, setVarPickerOpen] = useState(false);
  const [varSelected, setVarSelected] = useState(0);
  const urlInputRef = useRef<HTMLInputElement>(null);

  // Live URL — what's actually in the input box right now. Normally that
  // is just the path (the part after baseUrl); in 自定义 mode it is the
  // whole address. Sourced from the store-side draft (so it survives tab
  // switches) and falling back to the persisted detail URL.
  const liveUrl = draft?.url ?? detail?.url ?? '';

  // The endpoint's project, straight off the denormalised `rootId`.
  // The old three-table model needed collection → module → project;
  // the single tree carries the answer on every row.
  const project = useMemo(
    () => (endpoint ? allProjects.find((p) => p.id === endpoint.rootId) : undefined),
    [endpoint, allProjects],
  );
  // Ancestors for the `项目 › 模块 › 集合` breadcrumb at the top of the
  // page — same anchoring pattern the Settings panel uses
  // (`设置 › 连接`). Collected root-first by walking `parentId` up, so
  // nesting deeper than one collection needs no extra case here.
  const ancestors = useMemo(() => {
    if (!endpoint) return [];
    const chain: { id: string; name: string; type: string }[] = [];
    const seen = new Set<string>([endpoint.id]);
    let cur = endpoint.parentId ? allNodes.find((n) => n.id === endpoint!.parentId) : undefined;
    while (cur && !seen.has(cur.id)) {
      seen.add(cur.id);
      chain.unshift({ id: cur.id, name: cur.name, type: cur.kind });
      cur = cur.parentId ? allNodes.find((n) => n.id === cur!.parentId) : undefined;
    }
    return chain;
  }, [endpoint, allNodes]);
  // The env that owns the baseUrl for this endpoint. Honors the
  // detail's `environmentId` if set; otherwise falls back to the
  // workspace-wide active env (the "ENV dev" pill in the top bar) so
  // a fresh endpoint isn't left baseUrl-less just because it hasn't
  // been told to prefer a specific env yet.
  const endpointEnv = useMemo(() => {
    if (detail?.environmentId) {
      return allEnvironments.find((e) => e.id === detail.environmentId);
    }
    return workspaceEnv;
  }, [detail, allEnvironments, workspaceEnv]);

  // Effective baseUrl for this endpoint — drives both the picker label
  // and the join at send time.
  const resolved = useMemo(
    () =>
      resolveEndpointBaseUrl(
        detail ?? { environmentId: null, baseUrlDefinitionId: null, baseUrlOverride: null },
        endpointEnv,
        project,
      ),
    [detail, endpointEnv, project],
  );

  // Drive the picker: reflect what's currently stored on the endpoint.
  // When the user picks a different source, the parent commits it back
  // via `updateEndpointDetail` — no draft needed since baseUrl changes
  // are persisted immediately (no Save button).
  //
  // `baseUrlOverride` is three-state and the empty string is the point of
  // 自定义: `null` means "not in custom mode", `''` means "custom mode,
  // URL field carries the whole address". Testing it for truthiness (as
  // this used to) collapsed those two together, so clicking 自定义 wrote
  // `''`, the next render read it back as falsy, and the picker snapped
  // straight back to 项目默认 — the click appeared to do nothing.
  const selection: BaseUrlSelection = useMemo(() => {
    if (!detail) return { kind: 'default' };
    if (detail.baseUrlOverride !== null && detail.baseUrlOverride !== undefined) {
      return { kind: 'override' };
    }
    if (detail.baseUrlDefinitionId) {
      return { kind: 'definition', definitionId: detail.baseUrlDefinitionId };
    }
    return { kind: 'default' };
  }, [detail]);
  // 自定义 hands the URL field the whole address, so nothing may be
  // prepended to it. This is also what `resolved.baseUrl` collapses to —
  // an empty string joins to the path unchanged (see joinBaseUrl) — but
  // read it off the selection so the intent survives a legacy override
  // that still carries a host.
  const isCustomUrl = selection.kind === 'override';

  // ---------------------------------------------------------------------------
  // The effective request: the stored detail with the draft layered on top.
  //
  // Every request-config editor writes to the draft instead of the row, so
  // this is the single place that decides what "what the user sees" means.
  // Send, Save, the cURL preview and validation all read it — deriving it
  // once is what keeps those four from disagreeing about the request.
  //
  // URL / method are read the same way (see `liveUrl` / `method` above) and
  // are kept out of this object because the URL bar also needs the raw
  // values for its own picker and caret logic.
  // ---------------------------------------------------------------------------
  const effective = useMemo(
    () => ({
      params: draft?.params ?? detail?.params ?? [],
      headers: draft?.headers ?? detail?.headers ?? [],
      body: draft?.body ?? detail?.body ?? EMPTY_BODY,
      auth: draft?.auth ?? detail?.auth ?? EMPTY_AUTH,
      docs: draft?.docs ?? detail?.docs ?? '',
      tags: draft?.tags ?? detail?.tags ?? [],
    }),
    [draft, detail],
  );

  /** Route one editor's output into the draft (and thus the dirty flag). */
  const patchDraft = useCallback(
    (patch: Partial<EndpointDraft>) => {
      if (!endpoint) return;
      setEndpointDraft(endpoint.id, patch);
    },
    [endpoint, setEndpointDraft],
  );

  // ---------------------------------------------------------------------------
  // A draft's document fields vs a saved node's
  //
  // A draft has no `nodes` row, so `updateNode` on one would fail against a
  // row that does not exist — its document fields live in `draftEndpoints`
  // and reach the database through `saveDraftEndpoint`, not here. Routing
  // both spellings through one callback is what keeps the rename controls
  // from having to know which kind of endpoint they are sitting on.
  // ---------------------------------------------------------------------------
  const saveEndpointName = useCallback(
    (next: string) => {
      if (!endpoint) return;
      if (isDraft) updateDraftEndpoint(endpoint.id, { name: next });
      else updateNode(endpoint.id, { name: next });
    },
    [endpoint, isDraft, updateDraftEndpoint, updateNode],
  );

  const saveEndpointDescription = useCallback(
    (next: string) => {
      if (!endpoint) return;
      if (isDraft) updateDraftEndpoint(endpoint.id, { description: next });
      else updateNode(endpoint.id, { description: next || undefined });
    },
    [endpoint, isDraft, updateDraftEndpoint, updateNode],
  );

  // "新建即命名" — a fresh draft opens with its title already selected.
  //
  // The tree used to express this by pointing its `editing` state at the
  // row it had just written. There is no row any more, so the affordance
  // moved here, into the surface where the name is actually read. Without
  // it, "新建接口" would land on a tab reading 未命名接口 and put one more
  // click between the user and every endpoint they create.
  //
  // Latched by draft id rather than driven by the name: the condition is
  // "this draft still has the placeholder", so it would otherwise re-open
  // the editor every time the user navigates back to a draft they chose
  // to leave unnamed.
  const [namingDraft, setNamingDraft] = useState(false);
  const autoNameOpenedFor = useRef<string | null>(null);
  useEffect(() => {
    if (!isDraft || !endpoint) return;
    if (endpoint.name !== UNTITLED_ENDPOINT_NAME) return;
    if (autoNameOpenedFor.current === endpoint.id) return;
    autoNameOpenedFor.current = endpoint.id;
    setNamingDraft(true);
  }, [isDraft, endpoint]);

  // Pinned format error. See `lib/request-validation`: validation is an
  // action run by Send / Save / 格式化, and a passing run clears it. Local
  // to this component on purpose — it survives switching between Params /
  // Body / Auth (BodyEditor remounts, this does not), and the next
  // send/save re-checks whatever is on screen at that point.
  const [validationError, setValidationError] = useState<string | null>(null);

  /**
   * Run validation, pin or clear the badge, and report whether it passed.
   *
   * On failure it also moves the user to the tab that owns the problem —
   * pinning an error inside the body editor while they sit on Params is the
   * same invisibility the badge was introduced to end.
   */
  const runValidation = useCallback((): boolean => {
    const issue = validateRequest({ body: effective.body });
    setValidationError(issue?.message ?? null);
    if (issue) {
      if (issue.tab === 'body') setTab('body');
      return false;
    }
    return true;
  }, [effective.body]);

  // Entries for the env-var picker, derived from the endpoint's own
  // env (the one whose variables this endpoint resolves `{{x}}` against)
  // + the partial variable name the user has typed after the last `{{`.
  const allEnvEntries = useMemo(
    () =>
      endpointEnv
        ? Object.entries(endpointEnv.variables)
            .map(([k, v]) => ({ key: k, value: v }))
            .sort((a, b) => a.key.localeCompare(b.key))
        : [],
    [endpointEnv],
  );
  const partialVar = extractPartialVar(liveUrl);
  const filteredEntries = useMemo(() => {
    const q = (partialVar ?? '').toLowerCase();
    if (!q) return allEnvEntries;
    return allEnvEntries.filter((e) => (e.key ?? '').toLowerCase().includes(q));
  }, [allEnvEntries, partialVar]);
  // Header autocomplete harvests the user's own header names from every
  // loaded detail. `Object.values` allocates, so it's memoized against
  // the map's identity instead of being rebuilt on every render.
  const headerSuggestions = useMemo(
    () => buildHeaderSuggestions(Object.values(allEndpointDetails)),
    [allEndpointDetails],
  );

  const handleUrlChange = (next: string) => {
    if (!endpoint) return;
    setEndpointDraft(endpoint.id, { url: next });
    setVarPickerOpen(shouldShowVarPicker(next));
    if (!shouldShowVarPicker(next)) setVarSelected(0);
  };

  const insertVar = (key: string) => {
    // Replace the last `{{` and any partial variable name (up to `}}`,
    // whitespace, or another `{`) with the chosen key, closing the braces.
    if (!endpoint) return;
    const base = draft?.url ?? detail?.url ?? '';
    const next = base.replace(/\{\{[^}\s]*$/, `{{${key}}}`);
    setEndpointDraft(endpoint.id, { url: next });
    setVarPickerOpen(false);
    setVarSelected(0);
    // Restore focus + move caret to end of the inserted token.
    window.setTimeout(() => {
      const el = urlInputRef.current;
      if (!el) return;
      el.focus();
      const caret = next.length;
      try {
        el.setSelectionRange(caret, caret);
      } catch {
        /* number inputs etc. — not relevant for type=text */
      }
    }, 0);
  };

  // 自定义 turns the URL bar into the whole address, so put the caret
  // there with the old content selected — typing then replaces the old
  // path instead of appending to it. Deferred one tick so it runs after
  // the re-render that switches the mode (and after the popover closes).
  const focusUrlForFullAddress = () => {
    window.setTimeout(() => {
      const el = urlInputRef.current;
      if (!el) return;
      el.focus();
      try {
        el.select();
      } catch {
        /* not a text input — nothing to select */
      }
    }, 0);
  };

  const handleUrlKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (!varPickerOpen) return;
    // Let modifier-combos through so global hotkeys (Ctrl+Enter, Ctrl+S) still fire.
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (filteredEntries.length > 0) {
        setVarSelected((s) => (s + 1) % filteredEntries.length);
      }
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (filteredEntries.length > 0) {
        setVarSelected((s) => (s - 1 + filteredEntries.length) % filteredEntries.length);
      }
    } else if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      const entry = filteredEntries[varSelected] ?? filteredEntries[0];
      if (entry) insertVar(entry.key);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      setVarPickerOpen(false);
    }
  };

  // sync draft with active endpoint
  const method = draft?.method ?? detail?.method ?? 'GET';

  // The request as it will actually go on the wire, assembled **once**.
  //
  // Send and the cURL preview both read this same object, which is the
  // entire point of the arrangement. They used to build the URL
  // independently — Send in Rust (`variable_fill::resolve_endpoint`,
  // keyed off `endpointId`) and the preview here in TypeScript — and the
  // two had to be kept in step by hand. `handleCopyCurl` carried an
  // explicit workaround for the divergence: it had to pass the
  // hand-joined `resolvedUrl` into the overrides because
  // `buildCurlCommand` never joined a base URL on its own, so passing
  // the draft's raw path produced `curl -X POST '/cashier'` — a command
  // that cannot run.
  //
  // One construction core doesn't document that risk away, it deletes
  // the place it lived: the preview is now a serialisation of precisely
  // the bytes Send ships.
  const prepared = useMemo(
    () =>
      prepareRequest({
        method,
        urlPath: liveUrl,
        baseUrl: resolved.baseUrl,
        params: effective.params,
        headers: effective.headers,
        body: effective.body,
        auth: effective.auth,
        env: endpointEnv,
      }),
    [method, liveUrl, resolved.baseUrl, effective, endpointEnv],
  );

  // ---------------------------------------------------------------------------
  // Hooks that MUST run unconditionally. They used to sit after the
  // `if (!endpoint)` early return below, which is a rules-of-hooks
  // violation: switching away from an endpoint re-rendered this
  // component with fewer hooks and React threw "Rendered fewer hooks
  // than expected". Keep every hook above any early return.
  // ---------------------------------------------------------------------------

  // Abort any in-flight send when this component unmounts (tab
  // switch, project change, etc.) so the pending Promise can't
  // outlive the UI that requested it. `handleSend` reuses the same
  // ref so a second-click-while-pending becomes "cancel the previous
  // and start over" instead of a no-op race.
  const inFlightRef = useRef<AbortController | null>(null);
  useEffect(() => {
    return () => {
      inFlightRef.current?.abort();
      inFlightRef.current = null;
    };
  }, []);

  // C2: Send button morph.
  // `playIconRef` wraps the Play icon in an `inline-flex` span so we can
  // rotate it in place without disturbing the button text width.
  // `sendBtnRef` is the whole Button element — used for the micro-bounce
  // after a pending request resolves.
  const sendBtnRef = useRef<HTMLButtonElement>(null);
  const playIconRef = useRef<HTMLSpanElement>(null);

  useGSAP(() => {
    if (!playIconRef.current || !sendBtnRef.current) return;

    if (requestPending) {
      // Kill any leftover rotation (e.g. from a previous cancel) before
      // starting a fresh spin. gsap.fromTo with rotation +=360 +
      // repeat:-1 gives a continuous linear spin.
      gsap.killTweensOf(playIconRef.current);
      gsap.fromTo(
        playIconRef.current,
        { rotation: 0 },
        { rotation: '+=360', duration: 1, repeat: -1, ease: 'none' },
      );
    } else {
      // Resolve path: kill the spin and snap the icon back upright so
      // the next render starts at rotation 0. Then micro-bounce the
      // button to confirm the response arrived.
      gsap.killTweensOf(playIconRef.current);
      gsap.set(playIconRef.current, { rotation: 0 });
      gsap.killTweensOf(sendBtnRef.current);
      gsap.fromTo(
        sendBtnRef.current,
        { scale: 1 },
        {
          keyframes: [
            { scale: 1.05, duration: 0.125, ease: 'power2.out' },
            { scale: 1, duration: 0.125, ease: 'power2.in' },
          ],
        },
      );
    }
  }, [requestPending]);

  // C3: Copy-as-cURL feedback bounce. Fires when curlFlash transitions
  // false → true; the false transition is intentionally a no-op so the
  // already-swapped "已复制" label doesn't get an extra blip on its way out.
  const copyCurlBtnRef = useRef<HTMLButtonElement>(null);
  useGSAP(() => {
    if (!curlFlash || !copyCurlBtnRef.current) return;
    gsap.killTweensOf(copyCurlBtnRef.current);
    gsap.fromTo(
      copyCurlBtnRef.current,
      { scale: 1 },
      {
        keyframes: [
          { scale: 1.06, duration: 0.1, ease: 'power2.out' },
          { scale: 1, duration: 0.1, ease: 'power2.in' },
        ],
      },
    );
  }, [curlFlash]);

  // The breadcrumb answers "where am I in the tree" — but the moment the
  // endpoint owns a workspace tab, that tab is already showing the name
  // and the collection hint, so the path above it repeats what is one row
  // higher on the same screen. Read as a boolean out of the selector (so
  // no allocation) rather than derived from "is the tester being
  // rendered", so the crumb stays available on the path that has no tab
  // to carry the answer. Above the early return for the same
  // rules-of-hooks reason as everything else in this block.
  const endpointHasTab = useAppStore(
    (s) =>
      !!endpoint &&
      s.openTabs.some(
        // Both endpoint kinds count. A draft endpoint owns a tab exactly
        // like a saved one, and that tab is just as visible in the strip —
        // so matching on `'endpoint'` alone would have left the crumb on
        // screen for the one endpoint type most likely to be brand new.
        (t) => (t.kind === 'endpoint' || t.kind === 'endpoint-draft') && t.id === endpoint.id,
      ),
  );

  // A node with no detail row is a hole in the data, not a normal state
  // (every endpoint is created with one). Bail with the same empty
  // state rather than rendering a half-broken tester.
  if (!endpoint || !detail) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        从左侧选择一个接口开始
      </div>
    );
  }

  const enabledParams = effective.params.filter((p) => p.enabled && p.key).length;
  const enabledHeaders = effective.headers.filter((h) => h.enabled && h.key).length;
  const hasBody = effective.body.mode !== 'none' && effective.body.content;
  const hasAuth = effective.auth.kind !== 'none';

  const tabItems = TABS.map((t) => {
    let badge: React.ReactNode = null;
    if (t.id === 'params' && enabledParams)
      badge = <span className="rounded bg-muted px-1 text-[10px]">{enabledParams}</span>;
    if (t.id === 'headers' && enabledHeaders)
      badge = <span className="rounded bg-muted px-1 text-[10px]">{enabledHeaders}</span>;
    if (t.id === 'body' && hasBody)
      badge = <span className="rounded bg-primary/20 px-1 text-[10px] text-primary">●</span>;
    if (t.id === 'auth' && hasAuth)
      badge = (
        <span className="rounded bg-warning/20 px-1 text-[10px] text-warning-foreground">●</span>
      );
    return { id: t.id, label: t.label, badge, disabled: t.disabled };
  });

  const handleSend = async () => {
    if (!endpoint) return;
    // Validation gates the request. An unparseable json body is not
    // something the server should be asked to interpret, and sending it
    // anyway would leave the user staring at a server-side parse error
    // instead of the one naming the character they mistyped.
    if (!runValidation()) return;
    // Cancel any in-flight send first.
    inFlightRef.current?.abort();
    const ctrl = new AbortController();
    inFlightRef.current = ctrl;
    setRequestPending(true);
    try {
      // Everything about the request is already in `prepared` — the
      // backend is a pure executor and is never told which endpoint this
      // came from. That is what lets an unsaved draft send: there is no
      // `endpointId` to look up, so there is nothing to persist first.
      //
      // The ENV pill is Zustand-only and the database's `is_active` never
      // follows it, so resolution has to happen against what is on screen
      // (`endpointEnv`) — which `prepareRequest` already folded in.
      const resp = await sendPreparedRequest({
        request: prepared,
        settings: requestSettings,
        signal: ctrl.signal,
      });
      setLastResponse(resp);
      addResponseHistory(endpoint.id, resp);
    } catch (e) {
      // sendRequest is documented to never throw — this catch is a
      // defensive net in case buildFetchInit or similar misbehaves.
      console.error('[HttpTester] sendRequest threw:', e);
    } finally {
      // Only clear if we're still the active controller (a newer
      // send may have replaced us mid-flight).
      if (inFlightRef.current === ctrl) inFlightRef.current = null;
      setRequestPending(false);
    }
  };

  const handleSave = () => {
    if (!endpoint) return;
    // Same gate as Send: persisting a json body that does not parse would
    // store a broken request on purpose. A failed run leaves the draft in
    // place — nothing is written, nothing is cleared.
    if (!runValidation()) return;

    // An unsaved endpoint has no row to patch, so the whole write is one
    // step: `saveDraftEndpoint` merges the document fields and the pending
    // request-config edits into a single `detail`, writes it, and swaps
    // the draft tab for the real endpoint tab. It keeps the draft (and
    // this tab) when the write fails, so retrying is just pressing Save
    // again — the error surfaces as `draftSaveError`.
    if (isDraft) {
      void saveDraftEndpoint(endpoint.id);
      return;
    }

    const patch = draft ? draftToPatch(draft) : {};
    if (Object.keys(patch).length > 0) updateEndpointDetail(endpoint.id, patch);
    clearEndpointDraft(endpoint.id);
  };

  const handleCopyCurl = async () => {
    // One line, because there is nothing left to line up. The command is
    // rendered from `prepared` — the object Send ships — so it cannot
    // describe a different request than the one the user just watched
    // fire. Copying is not one of the validating paths, so an unparseable
    // body still yields a command: it is a copy, not a request, and
    // refusing to copy would only be in the way.
    const curl = preparedToCurl(prepared);
    try {
      await navigator.clipboard.writeText(curl);
      setCurlFlash(true);
      window.setTimeout(() => setCurlFlash(false), 1500);
    } catch (err) {
      // Permissions denied, insecure context, etc. — surface to the console
      // (no toast component yet) so the user can fall back to selecting text.
      console.error('[HttpTester] copy as cURL failed:', err);
    }
  };

  // A draft is unsaved by construction, so the button has to stay live even
  // when no editor has fired yet. Gating on `draft !== undefined` alone
  // would strand exactly the endpoint this feature exists for: one the
  // user created, typed a URL into, sent, and now wants to keep — with no
  // params / headers / body edits to make it dirty.
  const dirty = isDraft || draft !== undefined;

  return (
    <section className="flex h-full min-w-0 flex-1 flex-col">
      {/* Breadcrumb — mirrors the `设置 › 连接` pattern in the Settings
          panel. Gives the user a single-screen answer to "where am I in
          the tree". Hidden while a workspace tab carries the same
          information (see `endpointHasTab`). */}
      {!endpointHasTab && (
        <nav
          aria-label="接口所属路径"
          className="flex shrink-0 items-center gap-1 px-4 pt-5 text-[11px] text-muted-foreground"
        >
          {project && ancestors.length > 0 ? (
            <>
              <button
                type="button"
                onClick={() => {
                  const id = useAppStore.getState().activeProjectId;
                  // `openProject` rather than `setActiveProject`: the
                  // breadcrumb's first crumb is a link to the project, so
                  // it should bring up the project pane (and re-scope the
                  // tree), not just re-resolve the environment.
                  if (id) useAppStore.getState().openProject(id);
                }}
                className="truncate hover:text-foreground"
                title={project.name}
              >
                {project.name}
              </button>
              {ancestors.map((a) => (
                <span key={a.id} className="contents">
                  <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground/50" />
                  <span className="truncate" title={a.name}>
                    {a.name}
                  </span>
                </span>
              ))}
              <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground/50" />
              <span className="truncate text-foreground" title={endpoint.name}>
                {endpoint.name}
              </span>
            </>
          ) : (
            <span>接口</span>
          )}
        </nav>
      )}

      {/* Endpoint header */}
      <div className="flex shrink-0 items-start gap-2 border-b border-border px-4 pb-3 pt-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <InlineEdit
              key={`name-${endpoint.id}`}
              value={endpoint.name}
              onSave={saveEndpointName}
              testId="endpoint-name-input"
              // Controlled only for a draft, where `namingDraft` is what
              // opens the editor on a freshly created endpoint. Passing
              // `undefined` leaves a saved endpoint on the editor's own
              // double-click behaviour, untouched.
              editing={isDraft ? namingDraft : undefined}
              onEditingChange={(next) => {
                if (isDraft) setNamingDraft(next);
              }}
              className="max-w-md text-sm font-semibold"
              display={() => (
                <h1
                  className="cursor-text truncate rounded-sm px-1 py-0.5 text-sm font-semibold hover:bg-accent/60"
                  title="双击重命名"
                >
                  {endpoint.name}
                </h1>
              )}
            />
            <TagInput
              value={effective.tags}
              onChange={(tags) => patchDraft({ tags })}
              placeholder="+ 标签"
              className="min-w-[140px] max-w-[320px] border-transparent bg-transparent px-1 py-0.5 focus-within:ring-0 focus-within:ring-offset-0"
            />
          </div>
          <InlineEdit
            key={`desc-${endpoint.id}`}
            value={endpoint.description ?? ''}
            onSave={saveEndpointDescription}
            // Description legitimately allows clearing — without this, an
            // empty draft is treated as a cancel and the field keeps its old
            // value instead of going back to "no description".
            commitEmpty
            testId="endpoint-description-input"
            className="mt-0.5 h-6 max-w-2xl text-xs"
            placeholder="一句话说明这个接口的用途…"
            display={() => (
              <p
                className={cn(
                  'mt-0.5 cursor-text truncate rounded-sm px-1 text-xs text-muted-foreground',
                  'hover:bg-accent/60',
                  !endpoint.description && 'italic text-muted-foreground/50',
                )}
                title="双击编辑描述"
              >
                {endpoint.description || '双击添加描述…'}
              </p>
            )}
          />
        </div>
      </div>

      {/* URL bar */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border bg-card/30 px-4 py-2.5">
        <MethodPicker
          value={method}
          onChange={(m) => {
            if (endpoint) setEndpointDraft(endpoint.id, { method: m });
          }}
          className="border-0"
        />
        <BaseUrlPicker
          definitions={project?.baseUrlDefinitions ?? []}
          defaultDefinitionId={project?.defaultBaseUrlDefinitionId ?? null}
          resolvedBaseUrl={resolved.baseUrl}
          selection={selection}
          onSelectionChange={(next) => {
            if (!endpoint) return;
            if (next.kind === 'default') {
              // Clear both — null on baseUrlDefinitionId makes
              // resolveEndpointBaseUrl fall back to the project default.
              updateEndpointDetail(endpoint.id, {
                baseUrlDefinitionId: null,
                baseUrlOverride: null,
              });
            } else if (next.kind === 'definition') {
              updateEndpointDetail(endpoint.id, {
                baseUrlDefinitionId: next.definitionId,
                baseUrlOverride: null,
              });
            } else {
              // 自定义 — `''` is the mode marker, not a cleared value:
              // it is exactly what stops resolveEndpointBaseUrl (and the
              // Rust resolver) from falling back to the environment's
              // base URL and prefixing the address the user is about to
              // type. A row saved before this shipped may already carry a
              // host; keep it so re-picking 自定义 doesn't retarget an
              // existing request by surprise.
              const alreadyCustom = selection.kind === 'override';
              updateEndpointDetail(endpoint.id, {
                baseUrlOverride: detail.baseUrlOverride ?? '',
                baseUrlDefinitionId: null,
              });
              if (!alreadyCustom) focusUrlForFullAddress();
            }
          }}
        />
        <div className="relative flex min-w-0 flex-1 items-center">
          <Input
            ref={urlInputRef}
            value={liveUrl}
            onChange={(e) => handleUrlChange(e.target.value)}
            onKeyDown={handleUrlKeyDown}
            // 自定义 makes this field the whole address, so it has to say so: the
            // placeholder switches to a full URL and the ring marks the
            // mode — the picker sits to the left of the field the user is
            // actually focused on, so its label alone isn't the cue.
            className={cn(
              'h-9 w-full font-mono text-sm',
              isCustomUrl && 'border-primary/40 ring-1 ring-inset ring-primary/40',
            )}
            placeholder={
              isCustomUrl || !resolved.baseUrl ? 'https://api.example.com/path' : '/path'
            }
            title={
              isCustomUrl
                ? '自定义：直接使用这里填写的完整地址，不再拼接 baseUrl 前缀'
                : resolved.baseUrl || undefined
            }
            data-testid="url-input"
          />
        </div>
        <Button
          ref={sendBtnRef}
          onClick={handleSend}
          disabled={requestPending}
          className="h-9 gap-1.5 px-4"
          style={{ backgroundColor: methodColorVar(method) }}
          data-testid="send-btn"
        >
          <span ref={playIconRef} className="inline-flex">
            <Play className="h-3.5 w-3.5 fill-current" />
          </span>
          {requestPending ? '发送中…' : 'Send'}
        </Button>
        <Tooltip content={curlFlash ? '已复制' : '复制为 cURL'} side="bottom">
          <Button
            ref={copyCurlBtnRef}
            variant="outline"
            size="sm"
            onClick={handleCopyCurl}
            className={cn('h-9 gap-1.5', curlFlash && 'text-success')}
            data-testid="copy-curl-btn"
          >
            {curlFlash ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
            {curlFlash ? '已复制' : 'cURL'}
          </Button>
        </Tooltip>
        <Button
          variant={dirty ? 'default' : 'outline'}
          size="sm"
          onClick={handleSave}
          disabled={!dirty}
          className="h-9 gap-1.5"
          data-testid="save-btn"
        >
          <Save className="h-3.5 w-3.5" />
          {isDraft ? '保存接口' : 'Save'}
        </Button>
      </div>

      {/* A failed draft save keeps the draft and the tab, so retrying is
          just another Save — but the user has to know the first press did
          nothing, or the click reads as broken. `addNode` swallows the
          real error into `null` after logging it, so this is the only
          signal the UI can give; the retry affordance is the button. */}
      {draftSaveError && (
        <div
          role="alert"
          className="flex shrink-0 items-center gap-2 border-b border-border bg-destructive/10 px-4 py-1.5 text-xs text-destructive"
          data-testid="draft-save-error"
        >
          <span>{draftSaveError}</span>
          <button
            type="button"
            className="underline underline-offset-2 hover:no-underline"
            onClick={handleSave}
          >
            重试
          </button>
        </div>
      )}

      {/* Tabs — connected to the panel below: this row owns the only
          separator (no inner one on the strip), and the active tab fills
          itself with the panel's background so it covers that separator
          and curves into it. `bg-card/30` matches the URL bar above, and
          is what gives the active tab something to stand out from —
          without it the tab and the panel are the same colour and the
          whole effect is a missing line rather than a merged tab.
          `pt-1.5` with no `pb` keeps the strip's bottom edge flush with
          the separator, so `-mb-px` on the tab lands it on top. */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border bg-card/30 px-4 pt-1.5">
        <Tabs
          variant="connected"
          joinStripClass="bg-card/30"
          items={tabItems}
          value={tab}
          onChange={(id) => setTab(id as TabId)}
        />
        <div className="ml-auto text-[10px] text-muted-foreground">
          <span className="kbd">Ctrl</span> + <span className="kbd">Enter</span> 发送
        </div>
      </div>

      {/* Tab content */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {tab === 'params' && (
          <KeyValueEditor
            items={effective.params}
            onChange={(items) => patchDraft({ params: items })}
            keyPlaceholder="query 参数名"
            valuePlaceholder="值（支持 {{var}}）"
          />
        )}
        {tab === 'headers' && (
          <KeyValueEditor
            items={effective.headers}
            onChange={(items) => patchDraft({ headers: items })}
            keyPlaceholder="Header 名"
            valuePlaceholder="值"
            // Recent (project-wide harvest) merged with the built-in
            // list — see common-headers.ts::buildHeaderSuggestions.
            keySuggestions={headerSuggestions}
          />
        )}
        {tab === 'body' && (
          <BodyEditor
            value={effective.body}
            onChange={(body) => patchDraft({ body })}
            // Pinned by the parent, because validation is an action run
            // on Send / Save / 格式化 — not a value derived from typing.
            validationError={validationError}
            // 格式化 already parsed the content, so its result settles the
            // badge directly. Re-running `runValidation` here would check
            // the pre-format draft (this render's closure), which is the
            // content the user just failed to parse.
            onFormatResult={(result) => setValidationError(result.ok ? null : result.error)}
          />
        )}
        {tab === 'auth' && (
          <AuthEditor value={effective.auth} onChange={(auth) => patchDraft({ auth })} />
        )}
        {tab === 'docs' && (
          <DocsEditor value={effective.docs} onChange={(docs) => patchDraft({ docs })} />
        )}
        {tab === 'tests' && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            Tests 将在 v0.3 推出
          </div>
        )}
      </div>

      {/* Env var picker — anchored to the URL input. Positioned via
          getBoundingClientRect inside the picker so it follows the input
          on scroll / resize. */}
      <EnvVarPicker
        open={varPickerOpen}
        onClose={() => setVarPickerOpen(false)}
        entries={filteredEntries}
        selected={varSelected}
        onSelectedChange={setVarSelected}
        onSelect={(key) => insertVar(key)}
        anchorRef={urlInputRef}
        envName={endpointEnv?.name}
      />
    </section>
  );
}
