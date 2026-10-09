import { useMemo, useRef } from 'react';
import { FilePlus, Settings as SettingsIcon, BookOpen, Sparkles, Package } from 'lucide-react';
import { useAppStore, type Theme } from '@/stores/app-store';
import { cn, focusRing } from '@/lib/utils';
import { gsap, useGSAP } from '@/lib/gsap';

const THEME_ICON: Record<Theme, string> = {
  dark: '🌙',
  light: '☀️',
  system: '🖥️',
};

/** Per-project node tallies for the picker cards. */
interface ProjectTally {
  modules: number;
  collections: number;
  endpoints: number;
}

/**
 * Welcome / project-picker screen.
 *
 * Shown when `activeProjectId === ''` — i.e. no project has been entered
 * yet. That is the app's *startup* state: `bootstrap()` loads the data
 * and deliberately selects nothing (see the comment in
 * `stores/bootstrap-slice.ts`), so opening the app in either transport
 * lands here rather than inside whichever project happened to sort
 * first. Picking a card calls `openProject`, which selects the project
 * **and** activates its tab — the same action the top-bar selector
 * uses, so both entry paths converge on identical state.
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
  const nodes = useAppStore((s) => s.nodes);
  const loaded = useAppStore((s) => s.loaded);
  const theme = useAppStore((s) => s.theme);
  const openProject = useAppStore((s) => s.openProject);
  const openSettingsTab = useAppStore((s) => s.openSettingsTab);
  const openNewItem = useAppStore((s) => s.openNewItem);

  const containerRef = useRef<HTMLDivElement>(null);

  // Hero stagger — runs once every time the welcome screen appears
  // (i.e. when `activeProjectId` flips back to '' and the component
  // re-mounts).
  useGSAP(
    () => {
      const targets = containerRef.current?.querySelectorAll<HTMLElement>('[data-welcome-stagger]');
      if (!targets || targets.length === 0) return;
      gsap.from(targets, {
        opacity: 0,
        y: 8,
        duration: 0.32,
        stagger: 0.06,
        ease: 'power3.out',
      });
    },
    { scope: containerRef, dependencies: [] },
  );

  // One pass over the flat node list, bucketed by the denormalised
  // `rootId`. Derived here rather than in a selector: a selector that
  // allocates a new object per call re-renders forever under Zustand v5.
  const tallies = useMemo(() => {
    const out = new Map<string, ProjectTally>();
    for (const n of nodes) {
      const t = out.get(n.rootId) ?? { modules: 0, collections: 0, endpoints: 0 };
      if (n.kind === 'module') t.modules += 1;
      else if (n.kind === 'collection') t.collections += 1;
      else t.endpoints += 1;
      out.set(n.rootId, t);
    }
    return out;
  }, [nodes]);

  if (activeProjectId !== '') return null;

  const handleDocs = () => {
    // placeholder — full docs are planned for v0.2
    window.alert('文档中心：敬请期待\n\nv0.2 上线后将提供完整使用指南与快捷键手册。');
  };

  return (
    <div
      ref={containerRef}
      data-testid="welcome-screen"
      className="flex h-full w-full items-center justify-center overflow-y-auto bg-background p-6"
    >
      <div className="w-full max-w-2xl space-y-8 text-center">
        {/* Hero */}
        <div className="space-y-3">
          <div
            data-welcome-stagger
            className="mx-auto flex h-12 w-12 items-center justify-center rounded-xl bg-gradient-to-br from-primary to-hue-project shadow-lg"
          >
            <Sparkles className="h-6 w-6 text-white" />
          </div>
          <h1 data-welcome-stagger className="text-2xl font-semibold tracking-tight">
            {loaded && projects.length > 0 ? '选择一个项目' : '简单、强大的 API 调试工作台'}
          </h1>
          <p data-welcome-stagger className="mx-auto max-w-md text-sm text-muted-foreground">
            {loaded && projects.length > 0 ? (
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
            className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3"
            data-testid="welcome-project-list"
          >
            {projects.map((p) => {
              const t = tallies.get(p.id);
              return (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => openProject(p.id)}
                  className={cn(
                    'group flex flex-col items-start gap-2 rounded-lg border border-border bg-card/40 p-4 text-left transition-colors',
                    'hover:border-primary hover:bg-accent/40',
                    focusRing,
                  )}
                >
                  <div className="flex w-full items-center gap-2">
                    <span
                      className="h-2.5 w-2.5 shrink-0 rounded-sm"
                      style={{ backgroundColor: p.color }}
                    />
                    <span className="min-w-0 flex-1 truncate text-sm font-semibold" title={p.name}>
                      {p.name}
                    </span>
                  </div>
                  {p.description && (
                    <span className="line-clamp-2 w-full text-[11px] text-muted-foreground">
                      {p.description}
                    </span>
                  )}
                  <span className="mt-auto font-mono text-[10px] tabular-nums text-muted-foreground">
                    {t?.modules ?? 0} 模块 · {t?.collections ?? 0} 集合 · {t?.endpoints ?? 0} 接口
                  </span>
                </button>
              );
            })}
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
      <NewProjectCard onClick={onCreate} primary />
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
        'group flex flex-col items-start gap-2 rounded-lg border p-4 text-left transition-colors',
        focusRing,
        primary
          ? 'border-primary/50 bg-primary/5 hover:border-primary hover:bg-primary/10'
          : 'border-dashed border-border bg-card/20 hover:border-primary/50 hover:bg-accent/40',
      )}
    >
      <div
        className={cn(
          'flex h-8 w-8 items-center justify-center rounded-md transition-colors',
          primary
            ? 'bg-primary/15 text-primary'
            : 'bg-muted text-muted-foreground group-hover:text-foreground',
        )}
      >
        {primary ? <FilePlus className="h-5 w-5" /> : <Package className="h-4 w-4" />}
      </div>
      <div className="text-sm font-semibold">新建项目</div>
      <div className="text-[11px] text-muted-foreground">从零开始一个新工作区</div>
    </button>
  );
}
