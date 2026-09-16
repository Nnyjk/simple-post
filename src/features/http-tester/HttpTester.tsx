import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import {
  Play,
  Save,
  Copy,
  Trash2,
  Plus,
  Check,
  X,
  ChevronDown,
  ChevronRight,
  NotebookPen,
  Eye,
  Pencil,
  Columns2,
} from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import { useAppStore, useActiveEndpoint, useActiveEnvironment } from '@/stores/app-store';
import { Tabs } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { MethodPicker } from '@/components/ui/method-picker';
import { BaseUrlPicker, type BaseUrlSelection } from '@/components/ui/base-url-picker';
import { TagInput } from '@/components/ui/tag-input';
import { Tooltip } from '@/components/ui/tooltip';
import { type KeyValue, type HttpMethod, type Endpoint } from '@/types/domain';
import { methodColorVar, uid, cn } from '@/lib/utils';
import { joinBaseUrl, resolveEndpointBaseUrl, resolveVars } from '@/lib/url';
import { KeyValueEditor } from './KeyValueEditor';
import { BodyEditor } from './BodyEditor';
import { AuthEditor } from './AuthEditor';
import { DocsEditor } from './DocsEditor';
import { endpointToCurl } from './curl';
import { EnvVarPicker, extractPartialVar, shouldShowVarPicker } from './EnvVarPicker';

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
  const updateEndpoint = useAppStore((s) => s.updateEndpoint);
  const setRequestPending = useAppStore((s) => s.setRequestPending);
  const setLastResponse = useAppStore((s) => s.setLastResponse);
  const addResponseHistory = useAppStore((s) => s.addResponseHistory);
  const setEndpointDraft = useAppStore((s) => s.setEndpointDraft);
  const clearEndpointDraft = useAppStore((s) => s.clearEndpointDraft);
  const requestPending = useAppStore((s) => s.requestPending);

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
  const draft = useAppStore((s) =>
    endpoint ? s.endpointDrafts[endpoint.id] : undefined,
  );
  // Whenever the active endpoint changes, also reset the local notes
  // expansion so a long-form edit on endpoint A doesn't keep showing
  // on endpoint B. The notes value itself is per-endpoint and follows
  // the active endpoint. We also drop any in-flight name/description
  // edit so the user doesn't accidentally commit a draft into the
  // *next* endpoint after switching tabs mid-rename.
  const lastEndpointIdRef = useRef<string | null>(endpoint?.id ?? null);
  useEffect(() => {
    if (endpoint && endpoint.id !== lastEndpointIdRef.current) {
      lastEndpointIdRef.current = endpoint.id;
      setNotesOpen(false);
      setEditingName(false);
      setEditingDescription(false);
    } else if (!endpoint) {
      lastEndpointIdRef.current = null;
    }
  }, [endpoint]);

  // Inline notes expansion under the title bar. Local state — collapse on
  // endpoint switch is fine; the value lives in `endpoint.notes` so it
  // follows the user across tabs/sessions.
  const [notesOpen, setNotesOpen] = useState(false);
  const [notesMode, setNotesMode] = useState<'edit' | 'preview' | 'split'>('split');

  // Name / description double-click → inline edit. Mirrors the
  // SettingsHeader pattern: a boolean editing flag + a local draft,
  // Enter / blur commits, Esc reverts. The persisted value lives on
  // the endpoint itself, so we re-sync the draft whenever the active
  // endpoint changes (or when an outside source — the tree, another
  // tab — updates the name).
  const [editingName, setEditingName] = useState(false);
  const [nameDraft, setNameDraft] = useState('');
  const [editingDescription, setEditingDescription] = useState(false);
  const [descriptionDraft, setDescriptionDraft] = useState('');
  useEffect(() => {
    if (endpoint && !editingName) setNameDraft(endpoint.name);
  }, [endpoint, editingName]);
  useEffect(() => {
    if (endpoint && !editingDescription) {
      setDescriptionDraft(endpoint.description ?? '');
    }
  }, [endpoint, editingDescription]);

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
  const projectEnvs = useMemo(
    () => (project ? allEnvironments.filter((e) => e.projectId === project.id) : []),
    [allEnvironments, project],
  );
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
    return allEnvEntries.filter((e) =>
      (e.key ?? '').toLowerCase().includes(q),
    );
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
    if (t.id === 'params' && enabledParams) badge = <span className="rounded bg-muted px-1 text-[10px]">{enabledParams}</span>;
    if (t.id === 'headers' && enabledHeaders) badge = <span className="rounded bg-muted px-1 text-[10px]">{enabledHeaders}</span>;
    if (t.id === 'body' && hasBody) badge = <span className="rounded bg-blue-500/20 px-1 text-[10px] text-blue-300">●</span>;
    if (t.id === 'auth' && hasAuth) badge = <span className="rounded bg-amber-500/20 px-1 text-[10px] text-amber-300">●</span>;
    return { id: t.id, label: t.label, badge, disabled: t.disabled };
  });

  const handleSend = async () => {
    setRequestPending(true);
    // Mock request: synthesize a response after a short delay
    await new Promise((r) => setTimeout(r, 350 + Math.random() * 400));
    const ok = Math.random() > 0.2;
    const body = ok
      ? {
          code: 0,
          message: 'ok',
          data: {
            id: 'mock_' + Math.random().toString(36).slice(2, 8),
            timestamp: Date.now(),
            echo: { method, url: resolvedUrl },
          },
        }
      : {
          code: 1001,
          message: 'invalid params',
          errors: { phone: 'invalid format' },
        };
    const response = {
      status: ok ? 200 : 400,
      statusText: ok ? 'OK' : 'Bad Request',
      durationMs: 80 + Math.random() * 200,
      sizeBytes: JSON.stringify(body).length,
      headers: {
        'content-type': 'application/json; charset=utf-8',
        'x-request-id': 'req_' + Math.random().toString(36).slice(2, 10),
        'x-ratelimit-remaining': String(Math.floor(Math.random() * 500)),
      },
      body: JSON.stringify(body, null, 2),
      bodyJson: body,
      contentType: 'application/json',
      // request context — keeps the response self-describing so the
      // history strip and any future "replay" UI can show what was sent.
      method,
      url: resolvedUrl,
    };
    setLastResponse(response);
    addResponseHistory(endpoint.id, response);
    setRequestPending(false);
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
    const overrides: { url?: string; method?: HttpMethod } = {};
    if (draft?.url !== undefined) overrides.url = draft.url;
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
      {/* Endpoint header */}
      <div className="flex shrink-0 items-start gap-2 border-b border-border px-4 py-2.5">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            {editingName ? (
              <Input
                value={nameDraft}
                onChange={(e) => setNameDraft(e.target.value)}
                onBlur={() => {
                  const next = nameDraft.trim();
                  if (next && next !== endpoint.name) {
                    updateEndpoint(endpoint.id, { name: next });
                  } else {
                    setNameDraft(endpoint.name);
                  }
                  setEditingName(false);
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    // Commit + stop the Enter from reaching the TagInput
                    // sibling (which would otherwise interpret it as
                    // "add current value as a tag").
                    e.preventDefault();
                    e.stopPropagation();
                    (e.target as HTMLInputElement).blur();
                  } else if (e.key === 'Escape') {
                    e.preventDefault();
                    e.stopPropagation();
                    setNameDraft(endpoint.name);
                    setEditingName(false);
                  }
                }}
                className="h-7 max-w-md text-sm font-semibold"
                autoFocus
                data-testid="endpoint-name-input"
              />
            ) : (
              <h1
                onDoubleClick={() => setEditingName(true)}
                className="cursor-text truncate rounded-sm px-1 py-0.5 text-sm font-semibold hover:bg-accent/60"
                title="双击重命名"
              >
                {endpoint.name}
              </h1>
            )}
            <TagInput
              value={endpoint.tags}
              onChange={(tags) => updateEndpoint(endpoint.id, { tags })}
              placeholder="+ 标签"
              className="min-w-[140px] max-w-[320px] border-transparent bg-transparent px-1 py-0.5 focus-within:ring-0 focus-within:ring-offset-0"
            />
          </div>
          {/* Description — always-visible single-line input. Double-click
              also activates it; the same EditableDescriptionField shape
              is used so a stray click doesn't accidentally eat focus. */}
          {editingDescription ? (
            <Input
              value={descriptionDraft}
              onChange={(e) => setDescriptionDraft(e.target.value)}
              onBlur={() => {
                const next = descriptionDraft.trim();
                const current = endpoint.description ?? '';
                if (next !== current) {
                  updateEndpoint(endpoint.id, { description: next || undefined });
                } else {
                  setDescriptionDraft(current);
                }
                setEditingDescription(false);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  e.stopPropagation();
                  (e.target as HTMLInputElement).blur();
                } else if (e.key === 'Escape') {
                  e.preventDefault();
                  e.stopPropagation();
                  setDescriptionDraft(endpoint.description ?? '');
                  setEditingDescription(false);
                }
              }}
              className="mt-0.5 h-6 max-w-2xl text-xs"
              placeholder="一句话说明这个接口的用途…"
              autoFocus
              data-testid="endpoint-description-input"
            />
          ) : (
            <p
              onDoubleClick={() => setEditingDescription(true)}
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
        </div>
        <Tooltip
          content={notesOpen ? '收起详细说明' : '展开详细说明（Markdown）'}
          side="bottom"
        >
          <button
            type="button"
            aria-label={notesOpen ? '收起详细说明' : '展开详细说明'}
            aria-expanded={notesOpen}
            onClick={() => setNotesOpen((v) => !v)}
            className={cn(
              'flex h-6 shrink-0 items-center gap-1 rounded-md px-2 text-[10px] font-medium transition-colors',
              notesOpen
                ? 'bg-primary/15 text-primary'
                : 'text-muted-foreground hover:bg-accent/60 hover:text-foreground',
            )}
          >
            <NotebookPen className="h-3 w-3" />
            <span>详细说明</span>
            {notesOpen ? (
              <ChevronDown className="h-3 w-3" />
            ) : (
              <ChevronRight className="h-3 w-3" />
            )}
          </button>
        </Tooltip>
      </div>

      {/* Inline notes expansion. Sits between the header and the URL bar
          so it doesn't push the request editor out of view when closed. */}
      {notesOpen && (
        <EndpointNotesPanel
          value={endpoint.notes ?? ''}
          onChange={(notes) => updateEndpoint(endpoint.id, { notes })}
          mode={notesMode}
          onModeChange={setNotesMode}
        />
      )}

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
          onClick={handleSend}
          disabled={requestPending}
          className="h-9 gap-1.5 px-4"
          style={{ backgroundColor: methodColorVar(method) }}
          data-testid="send-btn"
        >
          <Play className="h-3.5 w-3.5 fill-current" />
          {requestPending ? '发送中…' : 'Send'}
        </Button>
        <Tooltip content={curlFlash ? '已复制' : '复制为 cURL'} side="bottom">
          <Button
            variant="outline"
            size="sm"
            onClick={handleCopyCurl}
            className={cn('h-9 gap-1.5', curlFlash && 'text-emerald-400')}
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

// ----------------------------------------------------------------
// EndpointNotesPanel — inline markdown editor for the title-bar expansion.
//
// Self-contained, so it can re-render independently from the request
// tester above. The parent owns the open/close state and the persisted
// `endpoint.notes` string; this component only handles the three
// edit/preview/split view modes.
// ----------------------------------------------------------------

function EndpointNotesPanel({
  value,
  onChange,
  mode,
  onModeChange,
}: {
  value: string;
  onChange: (next: string) => void;
  mode: 'edit' | 'preview' | 'split';
  onModeChange: (m: 'edit' | 'preview' | 'split') => void;
}) {
  // Track empty state separately so the placeholder copy can show.
  const empty = value.trim().length === 0;
  return (
    <div className="flex shrink-0 flex-col border-b border-border bg-card/30">
      <div className="flex items-center gap-1 px-4 py-1.5">
        <span className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
          详细说明
        </span>
        <span className="text-[10px] text-muted-foreground/60">
          Markdown · 用于补充 description
        </span>
        <div className="ml-auto inline-flex items-center rounded-md border border-border bg-background/40 p-0.5 text-[10px]">
          <ModeButton
            active={mode === 'edit'}
            onClick={() => onModeChange('edit')}
            icon={<Pencil className="h-3 w-3" />}
            label="编辑"
          />
          <ModeButton
            active={mode === 'split'}
            onClick={() => onModeChange('split')}
            icon={<Columns2 className="h-3 w-3" />}
            label="分屏"
          />
          <ModeButton
            active={mode === 'preview'}
            onClick={() => onModeChange('preview')}
            icon={<Eye className="h-3 w-3" />}
            label="预览"
          />
        </div>
      </div>
      <div
        className={cn(
          'grid min-h-[120px] max-h-[260px]',
          mode === 'split' ? 'grid-cols-2' : 'grid-cols-1',
        )}
      >
        {mode !== 'preview' && (
          <textarea
            value={value}
            onChange={(e) => onChange(e.target.value)}
            placeholder={'# 用法说明\n\n这个接口做什么、什么时候调用、需要注意什么…\n\n```js\n示例代码\n```'}
            spellCheck={false}
            className={cn(
              'min-h-[120px] max-h-[260px] resize-y bg-transparent px-4 py-2 font-mono text-[12px] leading-relaxed',
              'placeholder:text-muted-foreground/40 focus:outline-none',
              mode === 'split' && 'border-r border-border',
            )}
          />
        )}
        {mode !== 'edit' && (
          <div className="min-h-[120px] max-h-[260px] overflow-auto bg-background/30 px-4 py-2 text-xs leading-relaxed">
            {empty ? (
              <p className="text-muted-foreground/50">（暂无内容 — 在左侧编辑）</p>
            ) : (
              <div className="markdown-prose">
                <ReactMarkdown>{value}</ReactMarkdown>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function ModeButton({
  active,
  onClick,
  icon,
  label,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'inline-flex items-center gap-1 rounded-sm px-1.5 py-0.5 transition-colors',
        active
          ? 'bg-background text-foreground shadow-sm'
          : 'text-muted-foreground hover:text-foreground',
      )}
    >
      {icon}
      {label}
    </button>
  );
}
