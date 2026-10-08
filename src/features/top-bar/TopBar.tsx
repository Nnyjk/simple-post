import { useState, useRef, useEffect, useMemo, Suspense } from 'react';
import { Settings2, ChevronDown, Circle, Cpu, Plus, Search, Sparkles, Loader2 } from 'lucide-react';
import { useAppStore, useActiveProject, useActiveEnvironment } from '@/stores/app-store';
import { useMcpStatus } from '@/lib/use-mcp-status';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { CommandPalette } from '@/features/command-palette/CommandPalette-lazy';
import { cn, focusRing } from '@/lib/utils';
import { WindowControls } from './WindowControls';

/**
 * Tauri 2 quirk: `data-tauri-drag-region="true"` on a container makes
 * ANY mousedown inside it start a window-drag gesture. Interactive
 * children (buttons / inputs) silently lose their click handlers unless
 * they're explicitly opted out with `data-tauri-drag-region="false"`.
 *
 * Pattern used here:
 *   - <header> = the whole row, drag-enabled.
 *   - Every <button> inside = drag-disabled (we pass the prop through
 *     the Button component, which spreads ...props onto the native
 *     <button>).
 *
 * Permissions live in `src-tauri/capabilities/default.json` —
 * `core:window:allow-start-dragging` must be listed or the drag
 * gesture won't fire even with the attribute present.
 */
const NO_DRAG = { 'data-tauri-drag-region': 'false' } as const;

export function TopBar() {
  const projects = useAppStore((s) => s.projects);
  const environments = useAppStore((s) => s.environments);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const activeProject = useActiveProject();
  const activeEnv = useActiveEnvironment();
  const setActiveProject = useAppStore((s) => s.setActiveProject);
  const setActiveEnvironment = useAppStore((s) => s.setActiveEnvironment);
  const openSettingsTab = useAppStore((s) => s.openSettingsTab);
  const openNewItem = useAppStore((s) => s.openNewItem);

  // NOTE: `bootstrap()` used to be kicked off from right here, in this
  // effect, because TopBar is the first mounted child of App.tsx. That is
  // strictly too late: effects run after React has parsed and executed the
  // entire initial module graph and mounted the tree, so the backend round
  // trip could not start until the slowest thing on the page was already
  // done — the two costs were serialised instead of overlapping. It now
  // fires from `main.tsx`, before `createRoot`, which lets the IPC run
  // concurrently with mount and with the editor chunks warming up.
  // `bootstrap()` is idempotent and guards on `loaded || loading`, so the
  // call order relative to this component is not load-bearing.

  const envs = useMemo(
    () => environments.filter((e) => e.projectId === activeProjectId),
    [environments, activeProjectId],
  );

  const [projOpen, setProjOpen] = useState(false);
  const [envOpen, setEnvOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const projRef = useRef<HTMLDivElement>(null);
  const envRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      if (projRef.current && !projRef.current.contains(e.target as Node)) setProjOpen(false);
      if (envRef.current && !envRef.current.contains(e.target as Node)) setEnvOpen(false);
    };
    document.addEventListener('mousedown', onClick);
    return () => document.removeEventListener('mousedown', onClick);
  }, []);

  // Global Ctrl/Cmd + K opens the command palette. Works from any focus
  // target (including input/textarea) — the spec is explicit about this.
  //
  // `e.isComposing` guard: while the IME composition session is open
  // (e.g. typing a Chinese character via Pinyin) Chrome/Firefox still
  // dispatch a `keydown` for Ctrl-modifier keys. Without this guard,
  // the palette would pop up over the IME candidate window and the
  // user's `k` press would vanish into the palette. Skipping composition
  // lets the IME commit happen first.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.isComposing) return;
      if ((e.ctrlKey || e.metaKey) && (e.key === 'k' || e.key === 'K')) {
        e.preventDefault();
        setPaletteOpen((v) => !v);
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, []);

  return (
    <header
      data-tauri-drag-region="true"
      className="flex h-11 shrink-0 select-none items-center gap-2 border-b border-border bg-card/50 px-3"
    >
      {/* Logo — pure decoration, sits inside the drag region. */}
      <div className="flex items-center gap-2 pr-2">
        <div className="flex h-6 w-6 items-center justify-center rounded-md bg-gradient-to-br from-primary to-hue-project">
          <Sparkles className="h-3.5 w-3.5 text-white" />
        </div>
        <span className="text-sm font-semibold tracking-tight">Simple Post</span>
      </div>

      {/* Project switcher — disabled until bootstrap populates projects. */}
      <div ref={projRef} className="relative">
        <Button
          {...NO_DRAG}
          variant="ghost"
          size="sm"
          onClick={() => setProjOpen((v) => !v)}
          disabled={!activeProject}
          className="gap-1.5"
        >
          {activeProject ? (
            <>
              <span
                className="h-2 w-2 rounded-sm"
                style={{ backgroundColor: activeProject.color }}
              />
              <span className="w-[160px] truncate">{activeProject.name}</span>
            </>
          ) : (
            <>
              <Loader2 className="h-3 w-3 animate-spin" />
              <span className="w-[160px] truncate">加载中…</span>
            </>
          )}
          <ChevronDown className="h-3.5 w-3.5 opacity-60" />
        </Button>
        {projOpen && activeProject && (
          <div className="absolute left-0 top-full z-50 mt-1 w-64 animate-slide-in-from-top rounded-md border border-border bg-popover p-1 shadow-lg">
            <div className="px-2 py-1 label-caps">项目</div>
            {projects.map((p) => (
              <button
                {...NO_DRAG}
                type="button"
                key={p.id}
                onClick={() => {
                  setActiveProject(p.id);
                  setProjOpen(false);
                }}
                className={cn(
                  'flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm hover:bg-accent',
                  p.id === activeProject.id && 'bg-accent',
                )}
              >
                <span className="h-2 w-2 rounded-sm" style={{ backgroundColor: p.color }} />
                <div className="min-w-0 flex-1">
                  <div className="truncate">{p.name}</div>
                  {p.description && (
                    <div className="truncate text-[11px] text-muted-foreground">
                      {p.description}
                    </div>
                  )}
                </div>
              </button>
            ))}
            <div className="mt-1 border-t border-border pt-1">
              <button
                {...NO_DRAG}
                type="button"
                onClick={() => {
                  setProjOpen(false);
                  openNewItem({ type: 'project' });
                }}
                className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-sm text-muted-foreground hover:bg-accent hover:text-foreground"
              >
                <span className="text-base leading-none">+</span>
                新建项目
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Environment switcher */}
      <div ref={envRef} className="relative">
        <Button
          {...NO_DRAG}
          variant="ghost"
          size="sm"
          onClick={() => setEnvOpen((v) => !v)}
          className="gap-1.5"
        >
          <span className="text-[10px] uppercase tracking-wider text-muted-foreground">env</span>
          {/* Fixed width, not `max-w`: the title bar must not resize when
              the active name changes length. `max-w` only caps the long
              case, so a short name collapsed the whole control and made
              switching envs shove everything to its right sideways.
              `truncate` still ellipsises an over-long name inside it. */}
          <span className="w-[80px] truncate font-mono text-xs">{activeEnv?.name ?? '—'}</span>
          <ChevronDown className="h-3.5 w-3.5 opacity-60" />
        </Button>
        {envOpen && (
          <div className="absolute left-0 top-full z-50 mt-1 w-56 animate-slide-in-from-top rounded-md border border-border bg-popover p-1 shadow-lg">
            <div className="px-2 py-1 label-caps">环境</div>
            {envs.length === 0 ? (
              <>
                <div className="px-2 py-3 text-center text-xs text-muted-foreground">
                  该项目暂无环境
                </div>
                <div className="mt-1 border-t border-border pt-1">
                  <button
                    {...NO_DRAG}
                    type="button"
                    onClick={() => {
                      setEnvOpen(false);
                      openSettingsTab('env');
                    }}
                    className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm text-muted-foreground hover:bg-accent hover:text-foreground"
                  >
                    <Plus className="h-3.5 w-3.5" />
                    新建环境
                  </button>
                </div>
              </>
            ) : (
              <>
                {envs.map((env) => (
                  <button
                    {...NO_DRAG}
                    type="button"
                    key={env.id}
                    onClick={() => {
                      setActiveEnvironment(env.id);
                      setEnvOpen(false);
                    }}
                    className={cn(
                      'flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm hover:bg-accent',
                      env.id === activeEnv?.id && 'bg-accent',
                    )}
                  >
                    <Circle
                      className={cn(
                        'h-2 w-2',
                        env.id === activeEnv?.id
                          ? 'fill-success text-success'
                          : 'text-muted-foreground/40',
                      )}
                    />
                    <span className="font-mono text-xs">{env.name}</span>
                    {env.id === activeEnv?.id && (
                      <span className="ml-auto text-[10px] text-success">active</span>
                    )}
                  </button>
                ))}
                <div className="mt-1 border-t border-border pt-1">
                  <button
                    {...NO_DRAG}
                    type="button"
                    onClick={() => {
                      setEnvOpen(false);
                      openSettingsTab('env');
                    }}
                    className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm text-muted-foreground hover:bg-accent hover:text-foreground"
                  >
                    <Plus className="h-3.5 w-3.5" />
                    新建环境
                  </button>
                </div>
              </>
            )}
          </div>
        )}
      </div>

      {/* Search box — clicking opens the command palette. */}
      <button
        {...NO_DRAG}
        type="button"
        onClick={() => setPaletteOpen(true)}
        className={cn(
          'ml-2 flex h-7 w-72 cursor-pointer items-center gap-1.5 rounded-md border border-input bg-background px-2 text-left text-muted-foreground transition-colors hover:bg-accent/40 hover:text-foreground/80',
          focusRing,
        )}
        data-testid="global-search"
        aria-label="打开命令面板"
      >
        <Search className="h-3.5 w-3.5 shrink-0" />
        <span className="flex-1 truncate text-sm text-muted-foreground/80">
          搜索命令、接口、项目…
        </span>
        <span className="kbd shrink-0">Ctrl</span>
        <span className="kbd shrink-0">K</span>
      </button>

      <div className="ml-auto flex items-center gap-2">
        <McpStatusPill />
        <Tooltip content="设置" side="bottom">
          <Button
            {...NO_DRAG}
            variant="ghost"
            size="icon"
            onClick={() => openSettingsTab()}
            aria-label="设置"
          >
            <Settings2 className="h-4 w-4" />
          </Button>
        </Tooltip>
      </div>

      {/* Minimize / maximize / close. Pinned to the right edge so the
          window operations cluster where every desktop app puts them. */}
      <WindowControls />

      <Suspense fallback={null}>
        <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} />
      </Suspense>
    </header>
  );
}

/**
 * Top-right MCP status pill. Reads from the singleton daemon snapshot
 * (see `useMcpStatus`) — the polling loop lives in the store, so this
 * component is pure render: no `setInterval`, no `invoke`, no listener.
 *
 * Renamed from the prior `McpStatus` because the previous version
 * hosted its own 3s poll and needed both React state and refs to
 * coordinate cancellation. With the shared poll those are gone, and
 * the shorter name is a clearer signal to future readers.
 */
function McpStatusPill() {
  const mcp = useMcpStatus();

  const status = mcp?.status ?? 'stopped';
  const pid = mcp?.pid ?? null;
  const requestsHandled = mcp?.requestsHandled ?? 0;

  const running = status === 'running';
  const unhealthy = status === 'crashed' || status === 'unhealthy';
  const connecting = status === 'starting' || status === 'restarting';

  const dotClass = running ? 'bg-success' : unhealthy ? 'bg-destructive' : 'bg-muted-foreground';

  return (
    <Tooltip
      content={
        <div className="space-y-0.5 text-left">
          <div className="font-medium">
            MCP Server ·{' '}
            {running
              ? '运行中'
              : connecting
                ? '启动中'
                : unhealthy
                  ? '异常'
                  : mcp
                    ? '未启动'
                    : '连接中…'}
          </div>
          <div className="text-[10px] text-muted-foreground">
            stdio 模式{pid !== null ? ` · PID ${pid}` : ''}
            {requestsHandled > 0 ? ` · ${requestsHandled} reqs` : ''}
          </div>
        </div>
      }
      side="bottom"
    >
      <button
        {...NO_DRAG}
        type="button"
        className="inline-flex h-7 items-center gap-1.5 rounded-md border border-border bg-card px-2 text-xs hover:bg-accent"
      >
        <span className="relative flex h-2 w-2">
          <span
            className={cn(
              'absolute inset-0 animate-ping rounded-full',
              running ? 'bg-success' : unhealthy ? 'bg-destructive' : 'bg-muted-foreground',
            )}
            style={{ opacity: 0.4 }}
          />
          <span className={cn('relative inline-flex h-2 w-2 rounded-full', dotClass)} />
        </span>
        <Cpu className="h-3 w-3" />
        <span>MCP</span>
      </button>
    </Tooltip>
  );
}
