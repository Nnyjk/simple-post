import { useState, useRef, useEffect, useMemo, Suspense } from 'react';
import { Settings2, ChevronDown, Circle, Cpu, Plus, Search, Loader2, Braces } from 'lucide-react';
import { useAppStore, useActiveProject, useActiveEnvironment } from '@/stores/app-store';
import { useMcpStatus } from '@/lib/use-mcp-status';
import { AppIcon } from '@/components/ui/app-icon';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { CommandPalette } from '@/features/command-palette/CommandPalette-lazy';
import { cn, focusRing } from '@/lib/utils';
import { WindowControls } from '@/components/ui/window-controls';

/**
 * Tauri 2 的坑：容器上带 `data-tauri-drag-region="true"` 时，内部**任何** mousedown
 * 都会启动窗口拖拽；交互子元素（按钮 / 输入）必须显式用
 * `data-tauri-drag-region="false"` 退出，否则会静默丢失点击处理。
 *
 * 此处的做法：
 *   - `<header>` 整行开启拖拽。
 *   - 其中每个 `<button>` 都关闭拖拽（经 Button 组件透传，它把 ...props 展开到原生
 *     `<button>`）。
 *
 * 权限在 `src-tauri/capabilities/default.json` —— 必须列出
 * `core:window:allow-start-dragging`，否则即使有该属性拖拽也不会触发。
 */
const NO_DRAG = { 'data-tauri-drag-region': 'false' } as const;

export function TopBar() {
  const projects = useAppStore((s) => s.projects);
  const environments = useAppStore((s) => s.environments);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const activeProject = useActiveProject();
  const activeEnv = useActiveEnvironment();
  const openProject = useAppStore((s) => s.openProject);
  const loaded = useAppStore((s) => s.loaded);
  const setEnvironmentActive = useAppStore((s) => s.setEnvironmentActive);
  const openSettingsTab = useAppStore((s) => s.openSettingsTab);
  const openDynamicVariablesTab = useAppStore((s) => s.openDynamicVariablesTab);
  const openNewItem = useAppStore((s) => s.openNewItem);
  const leaveProject = useAppStore((s) => s.leaveProject);

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

  // 全局 Ctrl/Cmd + K 打开命令面板，任意焦点目标（含 input/textarea）下都要生效。
  //
  // `e.isComposing` 守卫：IME 组合会话进行中（例如用拼音打汉字）时，Chrome/Firefox
  // 仍会为 Ctrl 组合键派发 `keydown`。不加此守卫，面板会盖在候选窗上，用户那一下
  // `k` 就被面板吞掉。跳过组合态，让 IME 先上屏。
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
      {/* 字标 —— 同时是回欢迎页的入口（`leaveProject`）。点产品名回首页是惯例，且它是
          本条中唯一不控制别的东西的元素。 */}
      <Tooltip content="返回欢迎页" side="bottom">
        <button
          {...NO_DRAG}
          type="button"
          onClick={leaveProject}
          aria-label="返回欢迎页"
          data-testid="back-to-welcome"
          className={cn(
            'flex items-center gap-2 rounded-md px-1.5 py-1 transition-colors hover:bg-accent/40',
            focusRing,
          )}
        >
          <AppIcon className="h-6 w-6" />
          <span className="text-sm font-semibold tracking-tight">Simple Post</span>
        </button>
      </Tooltip>

      {/* 项目切换器 —— 项目的入口（它不在树里）。只在 bootstrap 拿到列表前禁用；尚未
          进入项目时显示「选择项目…」而非「加载中…」，因为全新启动正是欢迎页请用户挑选
          的状态。 */}
      <div ref={projRef} className="relative">
        <Button
          {...NO_DRAG}
          variant="ghost"
          size="sm"
          onClick={() => setProjOpen((v) => !v)}
          disabled={!loaded}
          className="gap-1.5"
        >
          {activeProject ? (
            <>
              <span
                className="h-2 w-2 rounded-sm"
                style={{ backgroundColor: activeProject.color }}
              />
              {/* 用 `max-w` 而非固定 `w`：项目名在这里只是短标签，预留满 160px 会在条上
                  留下刺眼的空隙。与环境位不同，它不会每次切换都变，故不值得为省白空间
                  换来 `max-w` 遇短名就收窄的行为。长名仍会封顶并加省略号。 */}
              <span className="max-w-[160px] truncate">{activeProject.name}</span>
            </>
          ) : (
            <>
              {loaded ? (
                <span className="max-w-[160px] truncate">选择项目…</span>
              ) : (
                <>
                  <Loader2 className="h-3 w-3 animate-spin" />
                  <span className="max-w-[160px] truncate">加载中…</span>
                </>
              )}
            </>
          )}
          <ChevronDown className="h-3.5 w-3.5 opacity-60" />
        </Button>
        {projOpen && loaded && projects.length > 0 && (
          <div className="absolute left-0 top-full z-50 mt-1 w-64 animate-slide-in-from-top rounded-md border border-border bg-popover p-1 shadow-lg">
            <div className="px-2 py-1 label-caps">项目</div>
            {projects.map((p) => (
              <button
                {...NO_DRAG}
                type="button"
                key={p.id}
                onClick={() => {
                  openProject(p.id);
                  setProjOpen(false);
                }}
                className={cn(
                  'flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm hover:bg-accent',
                  p.id === activeProject?.id && 'bg-accent',
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

      {/* 环境切换器。以已有项目为前提：列表是
          `environments.filter(e => e.projectId === activeProjectId)`，没有项目时它为空，
          下拉会对一个不存在的项目报「该项目暂无环境」，还给出必然失败的「新建环境」。
          `WorkspaceShell` 通常只在项目激活后挂载，此守卫另覆盖只有设置页的情形。 */}
      {activeProjectId !== '' && (
        <div ref={envRef} className="relative">
          <Button
            {...NO_DRAG}
            variant="ghost"
            size="sm"
            onClick={() => setEnvOpen((v) => !v)}
            className="gap-1.5"
          >
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">env</span>
            {/* 固定宽度而非 `max-w`：标题栏不得因当前环境名长短变化而改变尺寸。`max-w`
              只封顶长名，短名反而会把整个控件收窄，切换环境时把它右侧的一切推来推去。
              `truncate` 仍会在固定宽度内把过长名字省略。 */}
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
                        // 写库，不只是改本地状态：「当前环境」是整个工作区共享的一个事实，
                        // 而 MCP 侧读的是库里那行 `environments.is_active`。只改本地的话，
                        // 界面按 dev 发请求而 AI 客户端按 prod 发 —— 同一份数据两个答案。
                        void setEnvironmentActive(env.id);
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
      )}

      {/* 动态变量入口。定义挂在项目里，故它开的是**当前项目**的那一页。 */}
      <Tooltip content="动态变量" side="bottom">
        <Button
          {...NO_DRAG}
          variant="ghost"
          size="icon"
          onClick={() => openDynamicVariablesTab()}
          aria-label="动态变量"
        >
          <Braces className="h-4 w-4" />
        </Button>
      </Tooltip>

      {/* 搜索框 —— 点击打开命令面板。 */}
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

      {/* 最小化 / 最大化 / 关闭。钉在右缘，让窗口操作聚在桌面应用惯放的位置。 */}
      <WindowControls />

      <Suspense fallback={null}>
        <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} />
      </Suspense>
    </header>
  );
}

/**
 * 右上角 MCP 状态药丸。读共享的守护进程快照（见 `useMcpStatus`）—— 轮询循环在 store
 * 里，故本组件是纯渲染：无 `setInterval`、无 `invoke`、无监听。
 *
 * 它也是连接设置的入口：连接页提供的每个控件都在「设置 · 连接」下，故一个告诉你
 * daemon 挂了的圆点，应当能把你带到能启动它的地方。它必须是真正可点的 `<button>` ——
 * 无 `onClick` 的按钮比 `<span>` 更糟：它向键盘与读屏用户宣告可激活，却什么都不做。
 */
function McpStatusPill() {
  const mcp = useMcpStatus();
  const openSettingsTab = useAppStore((s) => s.openSettingsTab);

  const status = mcp?.status ?? 'stopped';
  const pid = mcp?.pid ?? null;
  const port = mcp?.port ?? null;
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
          {/* 传输方式写死 Streamable HTTP —— 药丸不能宣告它已不再说的传输。端口与连接页
              写进客户端配置的是同一个数字，故它是唯一值得额外呈现的信息。 */}
          <div className="text-[10px] text-muted-foreground">
            Streamable HTTP
            {port !== null ? ` · :${port}` : ''}
            {pid !== null ? ` · PID ${pid}` : ''}
            {requestsHandled > 0 ? ` · ${requestsHandled} reqs` : ''}
          </div>
          <div className="text-[10px] text-muted-foreground/70">点击打开「设置 · 连接」</div>
        </div>
      }
      side="bottom"
    >
      <button
        {...NO_DRAG}
        type="button"
        onClick={() => openSettingsTab('connection')}
        aria-label="MCP 状态 — 打开连接设置"
        className={cn(
          'inline-flex h-7 items-center gap-1.5 rounded-md border border-border bg-card px-2 text-xs hover:bg-accent',
          focusRing,
        )}
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
