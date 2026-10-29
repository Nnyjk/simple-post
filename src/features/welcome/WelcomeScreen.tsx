import { useEffect, useRef } from 'react';
import {
  FilePlus,
  Settings as SettingsIcon,
  BookOpen,
  Package,
  ArrowRight,
  Braces,
} from 'lucide-react';
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
 * Welcome —— 独立的起始界面。
 *
 * `activeProjectId === ''` 时显示，也就是应用的启动态：`bootstrap()` 只加载项目列表、
 * 刻意不选中任何项目（见 `stores/bootstrap-slice.ts`），故两种传输下打开应用都落在这里，
 * 而不是落进恰好排在最前的那个项目。
 *
 * 全窗口界面，不是面板
 * ---------------------------------
 * `App.tsx` 是用它*替代*工作区框架渲染的，故它在位时周围的 chrome 根本不存在：没有项目时
 * 树里只有「尚未选择项目」占位、环境切换器没有可归属的项目、标签条是空的。另一半见
 * `features/workspace/WorkspaceShell.tsx`。
 *
 * 由此带来的状态结论值得说一句：`openProject()` 翻 `activeProjectId`，切换是瞬时的。
 * 没有可跨越的过渡，因为没有共享布局可供过渡。
 *
 * 它不显示什么
 * ---------------------
 * 各项目的节点数。那意味着要读 `s.nodes` 并在一块唯一职责是「让用户挑项目」的屏幕上对
 * 整棵树跑 `useMemo` —— 而树是 bootstrap 的第二层，此屏绘制时通常还没到，计数会读成一个
 * 笃定的零。卡片显示名称与描述，那才是用户真正在比较的东西。
 *
 * 本组件「自我管理可见性」：项目激活时返回 `null`，故在 App.tsx 里可直接渲染而无需在
 * 调用处再写条件（纵深防御 —— App.tsx 也会短路渲染，但这道守卫把契约写明了）。
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

  // 主视觉的错峰入场 —— 走 CSS，不用 GSAP。
  //
  // 本屏是启动界面，静态 import 的任何东西都会进 entry chunk。用 `gsap.from()` 作用在
  // `[data-welcome-stagger]` 上就得静态引入 `@/lib/gsap`，那会把 GSAP core + CSSPlugin
  // （约 60 kB 未压缩）放到每次冷启动的关键路径上，只为让六个随后静止的元素动起来。
  // 该动画纯属装饰，故改用 CSS 表达（见 `globals.css` 的 `.welcome-enter`）。
  // `@/lib/gsap` 仍可从 workspace shell 触达，那边本来就在 lazy 边界之后。
  useEffect(() => {
    // 每次挂载跑一次，也就是每次进入跑一次：`activeProjectId` 翻回 '' 时组件会重挂载。
    const el = containerRef.current;
    if (!el) return;
    el.classList.add('welcome-enter');
    return () => el.classList.remove('welcome-enter');
  }, []);

  if (activeProjectId !== '') return null;

  const handleDocs = () => {
    // 占位 —— 完整文档计划在 v0.2 推出
    window.alert('文档中心：敬请期待\n\nv0.2 上线后将提供完整使用指南与快捷键手册。');
  };

  const hasProjects = loaded && projects.length > 0;

  return (
    <div
      ref={containerRef}
      data-testid="welcome-screen"
      className="flex h-full min-h-0 w-full flex-col overflow-hidden bg-background"
    >
      {/* 极简 chrome。本屏自己没有顶栏，但窗口无边框（tauri.conf.json 里
          `decorations: false`），故没有拖拽区和这三个按钮，应用就无法从这里移动、最大化
          或关闭。工作区表头里的其余部分 —— 项目切换、环境切换、搜索、MCP 药丸 —— 在没有
          项目时本就无从触达，而这正是本意。

          `WindowControls` 自己就把每个按钮排除在拖拽区之外，故这里别的元素都不必写
          `data-tauri-drag-region`。 */}
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
        {/* 比上方 hero 更宽，且在其下左对齐：项目列表是一列名字而非正文，它需要这份
            宽度，以及读起来像列表的参差右缘。 */}
        <div className="w-full max-w-xl space-y-8 text-center">
          {/* 主视觉 */}
          <div className="space-y-3">
            <div
              data-welcome-stagger
              className="mx-auto flex h-12 w-12 items-center justify-center rounded-xl bg-linear-to-br from-primary to-hue-project shadow-lg"
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
                <>组织项目 · 整理接口 · 发送请求 · 内置 MCP 让 AI 直接驱动你的工作区。</>
              )}
            </p>
          </div>

          {/* 项目选择器 */}
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
                    // 用户在比较的是名字，故卡片围绕它构造：整宽一行、名字突出、颜色条降级
                    // 为前导强调、描述作为下方次级文字。
                    'group flex w-full items-center gap-3.5 rounded-lg border border-border bg-card/40 px-4 py-3.5 text-left transition-colors',
                    'hover:border-primary hover:bg-accent/40',
                    focusRing,
                  )}
                >
                  {/* 颜色条住在一个固定 32px 的槽位里，而不是作为自身宽 6px 的 flex
                      子项。

                      本列表是单列卡片，眼睛把各标签读成同一条左缘，但前导槽位宽度并不相同：
                      颜色条是 `w-1.5`，下方的 新建项目 卡片则以 `w-8` 图标徽标起头。让颜色条
                      按自身宽度排版，会把同一列表里每个项目标签比 新建项目 标签右推 26px ——
                      实测 x=389 vs x=415。预留槽位即可修正这条边缘，又不必在细色条与圆形徽标
                      之间二选一。 */}
                  <span className="flex h-8 w-8 shrink-0 items-center" aria-hidden>
                    <span className="h-8 w-1.5 rounded-full" style={{ backgroundColor: p.color }} />
                  </span>
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
                  {/* 这是便利性提示而非数据 —— 卡片全部职责就是「进入这个」。 */}
                  <ArrowRight
                    className="h-4 w-4 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100"
                    aria-hidden
                  />
                </button>
              ))}
              <NewProjectCard onClick={() => openNewItem({ type: 'project' })} primary />
            </div>
          )}

          {/* 次要操作 */}
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
            {/* 动态变量挂在项目里（变量名项目内唯一），故没有项目时它开不了 ——
                这里只说明它在哪，不给一个点了没反应的按钮。 */}
            <span className="inline-flex items-center gap-1.5 rounded-md border border-border/60 bg-card/20 px-3 py-1.5 text-xs text-muted-foreground/70">
              <Braces className="h-3.5 w-3.5" />
              动态变量在项目内维护，打开项目后从顶栏进入
            </span>
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

          {/* 页脚，显示当前主题 */}
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
 * 首次运行态：还没有任何项目。可提供的只有创建这一条路径，故给它整张卡片，而不是让它
 * 成为一列并不存在的项目中的一行。
 */
function EmptyWorkspace({ onCreate }: { onCreate: () => void }) {
  return (
    <div className="space-y-3" data-welcome-stagger>
      {/* 刻意与它所替换的列表同宽。宽度属于容器（上方的 `max-w-xl`），不属于屏幕上
          这唯一一张卡片，故不要给它另加 `mx-auto max-w-sm` 之类的包裹。见 `NewProjectCard`
          上的 `w-full`。 */}
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
        // 与项目卡片同形，好让两者读起来是同一个列表。
        //
        // `w-full` 在这里显式写出：这个控件无论被哪个状态渲染，都保持同一尺寸。
        'group flex w-full items-center gap-3.5 rounded-lg border px-4 py-3.5 text-left transition-colors',
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
