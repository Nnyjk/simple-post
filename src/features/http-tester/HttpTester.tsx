import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { Play, Save, Copy, Check, ChevronRight } from 'lucide-react';
import { useAppStore, useActiveEndpoint, useActiveEnvironment } from '@/stores/app-store';
import { Tabs } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { InlineEdit } from '@/components/ui/InlineEdit';
import { MethodPicker } from '@/components/ui/method-picker';
import { BaseUrlPicker, type BaseUrlSelection } from '@/components/ui/base-url-picker';
import { TagInput } from '@/components/ui/tag-input';
import { Tooltip } from '@/components/ui/tooltip';
import { type HttpMethod, type Endpoint } from '@/types/domain';
import { methodColorVar, cn } from '@/lib/utils';
import { joinBaseUrl, resolveEndpointBaseUrl, resolveVars } from '@/lib/url';
import { sendEndpoint } from '@/lib/http';
import { KeyValueEditor } from './KeyValueEditor';
import { buildHeaderSuggestions } from '@/lib/common-headers';
import { BodyEditor } from './BodyEditor';
import { AuthEditor } from './AuthEditor';
import { DocsEditor } from './DocsEditor';
import { endpointToCurl } from './curl';
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

type TabId = 'params' | 'headers' | 'body' | 'auth' | 'docs' | 'tests';

export function HttpTester() {
  const endpoint = useActiveEndpoint();
  // Workspace-wide active env (the "ENV dev" pill in the top bar).
  // Used as the fallback env when the endpoint hasn't picked one of
  // its own — that way the baseUrl + variable resolution "just works"
  // for a fresh endpoint without forcing the user through a picker
  // first.
  const workspaceEnv = useActiveEnvironment();
  // Raw store slices — we'll derive the endpoint's project / project envs
  // locally with useMemo. Going through `useAppStore` for each one keeps
  // the Zustand v5 "no allocation in selectors" rule intact.
  const allCollections = useAppStore((s) => s.collections);
  const allModules = useAppStore((s) => s.modules);
  const allProjects = useAppStore((s) => s.projects);
  const allEnvironments = useAppStore((s) => s.environments);
  // Pulled for the Header key autocomplete — we want the user's own
  // recently-used header names to surface in the datalist alongside
  // the built-in common list (see buildHeaderSuggestions).
  const allEndpoints = useAppStore((s) => s.endpoints);
  const updateEndpoint = useAppStore((s) => s.updateEndpoint);
  const setRequestPending = useAppStore((s) => s.setRequestPending);
  const setLastResponse = useAppStore((s) => s.setLastResponse);
  const addResponseHistory = useAppStore((s) => s.addResponseHistory);
  const setEndpointDraft = useAppStore((s) => s.setEndpointDraft);
  const clearEndpointDraft = useAppStore((s) => s.clearEndpointDraft);
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

  // Live URL — what's actually in the input box right now. The input
  // now stores only the path (the part after baseUrl) — the full URL
  // Live URL — what's actually in the input box right now. Sourced
  // from the store-side draft (so it survives tab switches) and
  // falling back to the persisted endpoint URL.
  const liveUrl = draft?.url ?? endpoint?.url ?? '';

  // Resolve the endpoint's project + the project's environments. Both
  // are needed for the baseUrl picker.
  const project = useMemo(() => {
    if (!endpoint) return undefined;
    const c = allCollections.find((c) => c.id === endpoint.collectionId);
    if (!c) return undefined;
    const m = allModules.find((m) => m.id === c.moduleId);
    if (!m) return undefined;
    return allProjects.find((p) => p.id === m.projectId);
  }, [endpoint, allCollections, allModules, allProjects]);
  // Module / collection are resolved alongside the project so we can
  // show a `项目 › 模块 › 集合` breadcrumb at the top of the page —
  // same anchoring pattern the Settings panel uses (`设置 › 连接`).
  const moduleAndCollection = useMemo(() => {
    if (!endpoint) return null;
    const c = allCollections.find((c) => c.id === endpoint.collectionId);
    if (!c) return null;
    const m = allModules.find((m) => m.id === c.moduleId);
    if (!m) return null;
    return { module: m, collection: c };
  }, [endpoint, allCollections, allModules]);
  // The env that owns the baseUrl for this endpoint. Honors the
  // endpoint's `environmentId` if set; otherwise falls back to the
  // workspace-wide active env (the "ENV dev" pill in the top bar) so
  // a fresh endpoint isn't left baseUrl-less just because it hasn't
  // been told to prefer a specific env yet.
  const endpointEnv = useMemo(() => {
    if (endpoint?.environmentId) {
      return allEnvironments.find((e) => e.id === endpoint.environmentId);
    }
    return workspaceEnv;
  }, [endpoint, allEnvironments, workspaceEnv]);

  // Effective baseUrl for this endpoint — drives both the picker label
  // and the join at send time.
  const resolved = useMemo(
    () =>
      resolveEndpointBaseUrl(
        endpoint ?? { environmentId: null, baseUrlDefinitionId: null, baseUrlOverride: null },
        endpointEnv,
        project,
      ),
    [endpoint, endpointEnv, project],
  );

  // Drive the picker: reflect what's currently stored on the endpoint.
  // When the user picks a different source, the parent commits it back
  // via `updateEndpoint` — no draft needed since baseUrl changes are
  // persisted immediately (no Save button).
  const selection: BaseUrlSelection = useMemo(() => {
    if (!endpoint) return { kind: 'default' };
    if (endpoint.baseUrlOverride) return { kind: 'override' };
    if (endpoint.baseUrlDefinitionId) {
      return { kind: 'definition', definitionId: endpoint.baseUrlDefinitionId };
    }
    return { kind: 'default' };
  }, [endpoint]);
  const customValue = endpoint?.baseUrlOverride ?? '';

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
    const base = draft?.url ?? endpoint.url ?? '';
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
  const method = draft?.method ?? endpoint?.method ?? 'GET';

  // Full URL = join baseUrl + path, then resolve `{{var}}` templates
  // against the env's variables. Used by handleSend, handleCopyCurl and
  // the response preview.
  const resolvedUrl = useMemo(() => {
    if (!endpoint) return liveUrl;
    const joined = joinBaseUrl(resolved.baseUrl, liveUrl);
    return resolveVars(joined, endpointEnv?.variables);
  }, [liveUrl, endpointEnv, endpoint, resolved.baseUrl]);

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

  if (!endpoint) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        从左侧选择一个接口开始
      </div>
    );
  }

  const enabledParams = endpoint.params.filter((p) => p.enabled && p.key).length;
  const enabledHeaders = endpoint.headers.filter((h) => h.enabled && h.key).length;
  const hasBody = endpoint.body.mode !== 'none' && endpoint.body.content;
  const hasAuth = endpoint.auth.type !== 'none';

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
    // Cancel any in-flight send first.
    inFlightRef.current?.abort();
    const ctrl = new AbortController();
    inFlightRef.current = ctrl;
    setRequestPending(true);
    try {
      const resp = await sendEndpoint({
        endpointId: endpoint.id,
        // The input holds only the path; the backend joins the base URL,
        // substitutes `{{var}}` and reduces auth from the stored rows.
        urlOverride: liveUrl,
        // The ENV pill is Zustand-only, so the database's `is_active` does
        // not follow the user's selection. Send what is on screen.
        environmentId: endpointEnv?.id,
        method,
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
    const patch: Partial<Endpoint> = {};
    if (draft?.url !== undefined) patch.url = draft.url;
    if (draft?.method !== undefined) patch.method = draft.method;
    if (Object.keys(patch).length > 0) updateEndpoint(endpoint.id, patch);
    clearEndpointDraft(endpoint.id);
  };

  const handleCopyCurl = async () => {
    // Synthesize a "current" endpoint by overlaying any unsaved draft on top
    // of the stored one — this matches what Send would actually transmit.
    //
    // The URL override is `resolvedUrl` (base URL + path + `{{var}}`), not
    // the raw path. `buildCurlCommand` only runs `resolveTemplate` over
    // whatever it is given; it does not join a base URL, so passing
    // `draft.url` produced `curl -X POST '/cashier'` — not runnable. Send
    // no longer composes the URL on this side at all, so `resolvedUrl` is
    // now the only place the joined form is still needed, for the preview.
    const overrides: { url?: string; method?: HttpMethod } = {};
    overrides.url = resolvedUrl;
    if (draft?.method !== undefined) overrides.method = draft.method;
    const curl = endpointToCurl(endpoint, endpointEnv, overrides);
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

  const dirty = draft !== undefined;

  return (
    <section className="flex h-full min-w-0 flex-1 flex-col">
      {/* Breadcrumb — mirrors the `设置 › 连接` pattern in the Settings
          panel. Gives the user a single-screen answer to "where am I in
          the tree". */}
      <nav
        aria-label="接口所属路径"
        className="flex shrink-0 items-center gap-1 px-4 pt-5 text-[11px] text-muted-foreground"
      >
        {project && moduleAndCollection ? (
          <>
            <button
              type="button"
              onClick={() => {
                const id = useAppStore.getState().activeProjectId;
                if (id) useAppStore.getState().setActiveProject(id);
              }}
              className="truncate hover:text-foreground"
              title={project.name}
            >
              {project.name}
            </button>
            <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground/50" />
            <span className="truncate" title={moduleAndCollection.module.name}>
              {moduleAndCollection.module.name}
            </span>
            <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground/50" />
            <span className="truncate text-foreground" title={moduleAndCollection.collection.name}>
              {moduleAndCollection.collection.name}
            </span>
          </>
        ) : (
          <span>接口</span>
        )}
      </nav>

      {/* Endpoint header */}
      <div className="flex shrink-0 items-start gap-2 border-b border-border px-4 pb-3 pt-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <InlineEdit
              key={`name-${endpoint.id}`}
              value={endpoint.name}
              onSave={(next) => updateEndpoint(endpoint.id, { name: next })}
              testId="endpoint-name-input"
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
              value={endpoint.tags}
              onChange={(tags) => updateEndpoint(endpoint.id, { tags })}
              placeholder="+ 标签"
              className="min-w-[140px] max-w-[320px] border-transparent bg-transparent px-1 py-0.5 focus-within:ring-0 focus-within:ring-offset-0"
            />
          </div>
          <InlineEdit
            key={`desc-${endpoint.id}`}
            value={endpoint.description ?? ''}
            onSave={(next) => updateEndpoint(endpoint.id, { description: next || undefined })}
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
          customValue={customValue}
          onSelectionChange={(next) => {
            if (!endpoint) return;
            if (next.kind === 'default') {
              // Clear both — null on baseUrlDefinitionId makes
              // resolveEndpointBaseUrl fall back to the project default.
              updateEndpoint(endpoint.id, {
                baseUrlDefinitionId: null,
                baseUrlOverride: null,
              });
            } else if (next.kind === 'definition') {
              updateEndpoint(endpoint.id, {
                baseUrlDefinitionId: next.definitionId,
                baseUrlOverride: null,
              });
            } else {
              // override — keep the previous value if the user just
              // re-picked the row, otherwise blank it so they can
              // type a fresh host.
              updateEndpoint(endpoint.id, {
                baseUrlOverride: endpoint.baseUrlOverride ?? '',
                baseUrlDefinitionId: null,
              });
            }
          }}
          onCustomValueChange={(v) => {
            if (!endpoint) return;
            updateEndpoint(endpoint.id, { baseUrlOverride: v });
          }}
        />
        <div className="relative flex min-w-0 flex-1 items-center">
          <Input
            ref={urlInputRef}
            value={liveUrl}
            onChange={(e) => handleUrlChange(e.target.value)}
            onKeyDown={handleUrlKeyDown}
            className="h-9 w-full font-mono text-sm"
            placeholder={resolved.baseUrl ? '/path' : 'https://api.example.com/path'}
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
          Save
        </Button>
      </div>

      {/* Tabs */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-4 py-1.5">
        <Tabs items={tabItems} value={tab} onChange={(id) => setTab(id as TabId)} />
        <div className="ml-auto text-[10px] text-muted-foreground">
          <span className="kbd">Ctrl</span> + <span className="kbd">Enter</span> 发送
        </div>
      </div>

      {/* Tab content */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {tab === 'params' && (
          <KeyValueEditor
            items={endpoint.params}
            onChange={(items) => updateEndpoint(endpoint.id, { params: items })}
            keyPlaceholder="query 参数名"
            valuePlaceholder="值（支持 {{var}}）"
          />
        )}
        {tab === 'headers' && (
          <KeyValueEditor
            items={endpoint.headers}
            onChange={(items) => updateEndpoint(endpoint.id, { headers: items })}
            keyPlaceholder="Header 名"
            valuePlaceholder="值"
            // Recent (project-wide harvest) merged with the built-in
            // list — see common-headers.ts::buildHeaderSuggestions.
            keySuggestions={buildHeaderSuggestions(allEndpoints)}
          />
        )}
        {tab === 'body' && (
          <BodyEditor
            value={endpoint.body}
            onChange={(body) => updateEndpoint(endpoint.id, { body })}
          />
        )}
        {tab === 'auth' && (
          <AuthEditor
            value={endpoint.auth}
            onChange={(auth) => updateEndpoint(endpoint.id, { auth })}
          />
        )}
        {tab === 'docs' && (
          <DocsEditor
            value={endpoint.docs ?? ''}
            onChange={(docs) => updateEndpoint(endpoint.id, { docs })}
          />
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
