import { useState, useRef, useEffect, useMemo } from 'react';
import {
  Settings2,
  ChevronDown,
  Circle,
  Cpu,
  Power,
  Plus,
  Search,
  Sparkles,
} from 'lucide-react';
import { useAppStore, useActiveProject, useActiveEnvironment } from '@/stores/app-store';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { CommandPalette } from '@/features/command-palette/CommandPalette';
import { cn } from '@/lib/utils';

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
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && (e.key === 'k' || e.key === 'K')) {
        e.preventDefault();
        setPaletteOpen((v) => !v);
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, []);

  return (
    <header className="flex h-11 shrink-0 items-center gap-2 border-b border-border bg-card/50 px-3">
      {/* Logo */}
      <div className="flex items-center gap-2 pr-2">
        <div className="flex h-6 w-6 items-center justify-center rounded-md bg-gradient-to-br from-blue-500 to-purple-500">
          <Sparkles className="h-3.5 w-3.5 text-white" />
        </div>
        <span className="text-sm font-semibold tracking-tight">Simple Post</span>
      </div>

      {/* Project switcher */}
      <div ref={projRef} className="relative">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setProjOpen((v) => !v)}
          className="gap-1.5"
        >
          <span
            className="h-2 w-2 rounded-sm"
            style={{ backgroundColor: activeProject.color }}
          />
          <span className="max-w-[160px] truncate">{activeProject.name}</span>
          <ChevronDown className="h-3.5 w-3.5 opacity-60" />
        </Button>
        {projOpen && (
          <div className="absolute left-0 top-full z-50 mt-1 w-64 animate-slide-in-from-top rounded-md border border-border bg-popover p-1 shadow-lg">
            <div className="px-2 py-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
              项目
            </div>
            {projects.map((p) => (
              <button
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
                <span
                  className="h-2 w-2 rounded-sm"
                  style={{ backgroundColor: p.color }}
                />
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
          variant="ghost"
          size="sm"
          onClick={() => setEnvOpen((v) => !v)}
          className="gap-1.5"
        >
          <span className="text-[10px] uppercase tracking-wider text-muted-foreground">env</span>
          <span className="max-w-[80px] truncate font-mono text-xs">
            {activeEnv?.name ?? '—'}
          </span>
          <ChevronDown className="h-3.5 w-3.5 opacity-60" />
        </Button>
        {envOpen && (
          <div className="absolute left-0 top-full z-50 mt-1 w-56 animate-slide-in-from-top rounded-md border border-border bg-popover p-1 shadow-lg">
            <div className="px-2 py-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
              环境
            </div>
            {envs.length === 0 ? (
              <>
                <div className="px-2 py-3 text-center text-xs text-muted-foreground">
                  该项目暂无环境
                </div>
                <div className="mt-1 border-t border-border pt-1">
                  <button
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
                        env.id === activeEnv?.id ? 'fill-emerald-400 text-emerald-400' : 'text-muted-foreground/40',
                      )}
                    />
                    <span className="font-mono text-xs">{env.name}</span>
                    {env.id === activeEnv?.id && (
                      <span className="ml-auto text-[10px] text-emerald-400">active</span>
                    )}
                  </button>
                ))}
                <div className="mt-1 border-t border-border pt-1">
                  <button
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

      {/* Search box — clicking opens the command palette. The visible kbd
          hint (Ctrl K) is the affordance for the global hotkey. */}
      <button
        type="button"
        onClick={() => setPaletteOpen(true)}
        className="ml-2 flex h-7 w-72 cursor-pointer items-center gap-1.5 rounded-md border border-input bg-background px-2 text-left text-muted-foreground transition-colors hover:bg-accent/40 hover:text-foreground/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background"
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
        <McpStatus />
        <Tooltip content="设置" side="bottom">
          <Button
            variant="ghost"
            size="icon"
            onClick={() => openSettingsTab()}
            aria-label="设置"
          >
            <Settings2 className="h-4 w-4" />
          </Button>
        </Tooltip>
      </div>

      <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} />
    </header>
  );
}

function McpStatus() {
  // mock: MCP "running" — this will be wired to Rust in next phase
  const running = true;
  return (
    <Tooltip
      content={
        running ? (
          <div className="space-y-0.5 text-left">
            <div className="font-medium">MCP Server · 运行中</div>
            <div className="text-[10px] text-muted-foreground">stdio 模式 · PID 4720</div>
          </div>
        ) : (
          <span>MCP Server 未启动</span>
        )
      }
      side="bottom"
    >
      <button className="inline-flex h-7 items-center gap-1.5 rounded-md border border-border bg-card px-2 text-xs hover:bg-accent">
        <span className="relative flex h-2 w-2">
          <span
            className={cn(
              'absolute inset-0 animate-ping rounded-full',
              running ? 'bg-emerald-400' : 'bg-zinc-500',
            )}
            style={{ opacity: 0.4 }}
          />
          <span
            className={cn(
              'relative inline-flex h-2 w-2 rounded-full',
              running ? 'bg-emerald-400' : 'bg-zinc-500',
            )}
          />
        </span>
        <Cpu className="h-3 w-3" />
        <span>MCP</span>
      </button>
    </Tooltip>
  );
}
