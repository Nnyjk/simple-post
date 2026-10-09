import { useEffect, useRef } from 'react';
import { FilePlus, Settings as SettingsIcon, BookOpen, Package, ArrowRight } from 'lucide-react';
import { useAppStore, type Theme } from '@/stores/app-store';
import { cn, focusRing } from '@/lib/utils';
import { AppIcon } from '@/components/ui/app-icon';
import { WindowControls } from '@/components/ui/window-controls';

const THEME_ICON: Record<Theme, string> = {
  dark: '🌙',
  light: '☀️',
  system: '🖥️',
};

/**
 * Welcome — the standalone start screen.
 *
 * Shown whenever `activeProjectId === ''`, which is the app's startup
 * state: `bootstrap()` loads the project list and deliberately selects
 * nothing (see `stores/bootstrap-slice.ts`), so opening the app in either
 * transport lands here rather than inside whichever project happened to
 * sort first.
 *
 * A FULL-WINDOW SURFACE, NOT A PANE
 * ---------------------------------
 * `App.tsx` renders this screen *instead of* the workspace frame, so the
 * chrome around it does not exist while it is up. Without a project the
 * tree had nothing to show but a "尚未选择项目" placeholder, the env
 * switcher had no project to scope to, and the tab strip was empty. See
 * `features/workspace/WorkspaceShell.tsx` for the other half.
 *
 * The consequence for state is small and worth stating: `openProject()`
 * flips `activeProjectId` and the swap is immediate. There is no
 * transition to animate across, because there is no shared layout to
 * transition within.
 *
 * WHAT IT DOES NOT SHOW
 * ---------------------
 * Per-project node counts. Those would mean reading `s.nodes` and running a
 * `useMemo` over the entire tree on a screen whose only job is to let the
 * user pick a project — and since the tree is layer 2 of the bootstrap, it
 * has usually not arrived when this screen paints, so the counts would read
 * as a confident zero. The card shows the name and description, which is
 * what the user is actually choosing between.
 *
 * The component is "visibility-self-managed": it returns `null` whenever
 * a project is active, so it can be safely rendered in App.tsx without
 * extra conditional logic at the call-site. (Defence-in-depth — App.tsx
 * also short-circuits the render, but this guard makes the contract
 * explicit.)
 */
export function WelcomeScreen() {
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const projects = useAppStore((s) => s.projects);
  const loaded = useAppStore((s) => s.loaded);
  const theme = useAppStore((s) => s.theme);
  const openProject = useAppStore((s) => s.openProject);
  const openSettingsTab = useAppStore((s) => s.openSettingsTab);
  const openNewItem = useAppStore((s) => s.openNewItem);

  const containerRef = useRef<HTMLDivElement>(null);

  // Hero stagger — CSS, not GSAP.
  //
  // This screen is the startup surface, so anything it statically imports
  // lands in the entry chunk. `gsap.from()` over `[data-welcome-stagger]`
  // meant a static `@/lib/gsap` import, which put GSAP core + CSSPlugin
  // (~60 kB raw) on the critical path of every cold start to animate six
  // elements that then sit still. The animation is pure decoration, so it
  // is expressed in CSS instead (see `globals.css`, `.welcome-enter`).
  // `@/lib/gsap` stays reachable from the workspace shell, where it is
  // already behind a lazy boundary.
  useEffect(() => {
    // Runs once per mount, which is also once per entry: the component
    // re-mounts when `activeProjectId` flips back to ''.
    const el = containerRef.current;
    if (!el) return;
    el.classList.add('welcome-enter');
    return () => el.classList.remove('welcome-enter');
  }, []);

  if (activeProjectId !== '') return null;

  const handleDocs = () => {
    // placeholder — full docs are planned for v0.2
    window.alert('文档中心：敬请期待\n\nv0.2 上线后将提供完整使用指南与快捷键手册。');
  };

  const hasProjects = loaded && projects.length > 0;

  return (
    <div
      ref={containerRef}
      data-testid="welcome-screen"
      className="flex h-full min-h-0 w-full flex-col overflow-hidden bg-background"
    >
      {/* Minimal chrome. This screen has no top bar of its own, but the
          window is undecorated (`decorations: false` in tauri.conf.json),
          so without a drag region and these three buttons the app could
          not be moved, maximised or closed from here. Everything else the
          workspace header carries — project switcher, env switcher, search,
          MCP pill — is unreachable without a project, which is the point.

          `WindowControls` opts each of its buttons out of the drag region
          itself, so nothing else here needs `data-tauri-drag-region`. */}
      <header
        data-tauri-drag-region="true"
        className="flex h-11 shrink-0 select-none items-center gap-2 border-b border-border bg-card/50 px-3"
      >
        <div className="flex items-center gap-2">
          <AppIcon className="h-6 w-6" />
          <span className="text-sm font-semibold tracking-tight">Simple Post</span>
        </div>
        <div className="ml-auto flex items-center">
          <WindowControls />
        </div>
      </header>

      <main className="flex min-h-0 flex-1 items-center justify-center overflow-y-auto p-6">
        {/* Wider than the hero's measure and left-aligned below it: the
            project list is a column of names, not prose, so it wants the
            room and the ragged-right edge that reads as a list. */}
        <div className="w-full max-w-xl space-y-8 text-center">
          {/* Hero */}
          <div className="space-y-3">
            <div
              data-welcome-stagger
              className="mx-auto flex h-12 w-12 items-center justify-center rounded-xl bg-gradient-to-br from-primary to-hue-project shadow-lg"
            >
              <AppIcon className="h-7 w-7" />
            </div>
            <h1 data-welcome-stagger className="text-2xl font-semibold tracking-tight">
              {hasProjects ? '选择一个项目' : '简单、强大的 API 调试工作台'}
            </h1>
            <p data-welcome-stagger className="mx-auto max-w-md text-sm text-muted-foreground">
              {hasProjects ? (
                <>进入项目后，左侧只展示该项目的模块、集合与接口。</>
              ) : (
                <>组织项目 · 编排接口 · 发送请求 · 内置 MCP 让 AI 直接驱动你的工作区。</>
              )}
            </p>
          </div>

          {/* Project picker */}
          {!loaded ? (
            <p data-welcome-stagger className="text-sm text-muted-foreground">
              加载中…
            </p>
          ) : projects.length === 0 ? (
            <EmptyWorkspace onCreate={() => openNewItem({ type: 'project' })} />
          ) : (
            <div
              data-welcome-stagger
              className="grid gap-2.5 text-left sm:grid-cols-1"
              data-testid="welcome-project-list"
            >
              {projects.map((p) => (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => openProject(p.id)}
                  className={cn(
                    // The name is what the user is choosing between, so the
                    // card is built around it: a full-width row, name
                    // promoted, colour chip demoted to a leading accent,
                    // description as secondary text below.
                    'group flex w-full items-center gap-3.5 rounded-lg border border-border bg-card/40 px-4 py-3.5 text-left transition-colors',
                    'hover:border-primary hover:bg-accent/40',
                    focusRing,
                  )}
                >
                  <span
                    className="h-8 w-1.5 shrink-0 rounded-full"
                    style={{ backgroundColor: p.color }}
                    aria-hidden
                  />
                  <span className="min-w-0 flex-1">
                    <span
                      className="block truncate text-base font-semibold leading-snug"
                      title={p.name}
                    >
                      {p.name}
                    </span>
                    {p.description && (
                      <span className="mt-0.5 block truncate text-xs text-muted-foreground">
                        {p.description}
                      </span>
                    )}
                  </span>
                  {/* Affordance, not data — the card's whole job is "enter this". */}
                  <ArrowRight
                    className="h-4 w-4 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100"
                    aria-hidden
                  />
                </button>
              ))}
              <NewProjectCard onClick={() => openNewItem({ type: 'project' })} primary />
            </div>
          )}

          {/* Secondary actions */}
          <div data-welcome-stagger className="flex items-center justify-center gap-2">
            <button
              type="button"
              onClick={() => openSettingsTab()}
              className={cn(
                'inline-flex items-center gap-1.5 rounded-md border border-border bg-card/40 px-3 py-1.5 text-xs text-muted-foreground transition-colors hover:border-primary/40 hover:bg-accent/40 hover:text-foreground',
                focusRing,
              )}
            >
              <SettingsIcon className="h-3.5 w-3.5" />
              打开设置
            </button>
            <button
              type="button"
              onClick={handleDocs}
              className={cn(
                'inline-flex items-center gap-1.5 rounded-md border border-border bg-card/40 px-3 py-1.5 text-xs text-muted-foreground transition-colors hover:border-primary/40 hover:bg-accent/40 hover:text-foreground',
                focusRing,
              )}
            >
              <BookOpen className="h-3.5 w-3.5" />
              查看文档
            </button>
          </div>

          {/* Footnote with current theme indicator */}
          <div
            data-welcome-stagger
            className="flex items-center justify-center gap-1.5 text-[11px] text-muted-foreground/70"
          >
            <span>当前主题：</span>
            <span className="font-mono">
              {theme}
              <span className="ml-1" aria-hidden>
                {THEME_ICON[theme]}
              </span>
            </span>
            <span className="mx-1.5">·</span>
            <span>可在设置中切换</span>
          </div>
        </div>
      </main>
    </div>
  );
}

/**
 * First-run state: no project exists yet. The create path is the only
 * thing to offer, so it gets the whole card rather than a row among
 * projects that aren't there.
 */
function EmptyWorkspace({ onCreate }: { onCreate: () => void }) {
  return (
    <div className="space-y-3" data-welcome-stagger>
      {/* The only card on screen in the first-run state, so it centres
          itself rather than stretching to the list width. */}
      <div className="mx-auto max-w-sm">
        <NewProjectCard onClick={onCreate} primary />
      </div>
      <p className="text-xs text-muted-foreground">还没有任何项目，先创建一个开始使用。</p>
    </div>
  );
}

function NewProjectCard({ onClick, primary }: { onClick: () => void; primary?: boolean }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        // Same row shape as a project card so the two read as one list.
        'group flex items-center gap-3.5 rounded-lg border px-4 py-3.5 text-left transition-colors',
        focusRing,
        primary
          ? 'border-primary/50 bg-primary/5 hover:border-primary hover:bg-primary/10'
          : 'border-dashed border-border bg-card/20 hover:border-primary/50 hover:bg-accent/40',
      )}
    >
      <span
        className={cn(
          'flex h-8 w-8 shrink-0 items-center justify-center rounded-full transition-colors',
          primary
            ? 'bg-primary/15 text-primary'
            : 'bg-muted text-muted-foreground group-hover:text-foreground',
        )}
      >
        {primary ? <FilePlus className="h-4 w-4" /> : <Package className="h-4 w-4" />}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-base font-semibold leading-snug">新建项目</span>
        <span className="mt-0.5 block text-xs text-muted-foreground">从零开始一个新工作区</span>
      </span>
    </button>
  );
}
