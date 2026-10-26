import { Suspense, lazy, useEffect } from 'react';
import { WelcomeScreen } from '@/features/welcome/WelcomeScreen';
import { useAppStore, SETTINGS_TAB_ID, DYNAMIC_TAB_ID } from '@/stores/app-store';
import { BootSplashGate } from '@/features/welcome/BootSplashGate';

// ---------------------------------------------------------------------------
// 两个 surface，一个 store，按 `activeProjectId` 分流：
//
//   activeProjectId === ''  → WelcomeScreen   （全窗口，无顶栏/树/标签）
//   otherwise               → WorkspaceShell  （顶栏 + 树 + 标签）
//
// 设置是唯一的例外：settings 单例同时也是一个 tab，无项目时必须仍可达，
// 否则欢迎页的「打开设置」会写出一个没人渲染的 tab。此时渲染
// `SettingsSurface` —— 与 workspace 内同一份 `SettingsPanel`，只是外壳不同。
// 判断放在本文件而非 `WelcomeScreen` 内，才能保证两个宿主共用一份面板实现。
//
// 不在共享布局里做条件渲染：欢迎页会被挤进一个窗格，四周是无法在没有项目
// 时做任何事的 chrome。
//
// 收益：workspace（顶栏、项目树、HTTP tester、设置面板，以及经由它们的
// CodeMirror 编辑器）全部 `lazy()`，冷启动只拉 store 与项目选择器。
//
// 全局部分留在本文件而非任一 shell：主题、Ctrl+Enter / Ctrl+S 快捷键
// （按钮归 workspace，但监听器必须跨 surface 存活）、以及两个 surface
// 都能触发的浮层。
// ---------------------------------------------------------------------------

// 整个 workspace 外壳：chunk 内含树、顶栏、HTTP tester、设置面板，
// 以及经由它们的 CodeMirror 编辑器 —— 没有项目就都到不了。
const WorkspaceShell = lazy(() =>
  import('@/features/workspace/WorkspaceShell').then((m) => ({ default: m.WorkspaceShell })),
);

// 设置的独立宿主，仅在没有项目可挂载 tab 时才挂载。与 workspace 的
// settings tab 分属不同 chunk：两者都是 `lazy()`，这个必须留在欢迎页首屏之外。
// 面板体共用，只有外壳不同（见 `features/settings/SettingsSurface.tsx`）。
const SettingsSurface = lazy(() =>
  import('@/features/settings/SettingsSurface').then((m) => ({ default: m.SettingsSurface })),
);

// 动态变量同样是全局资产，故同样需要一个无项目宿主。面板体与 workspace 里的是同一份
// （见 `features/dynamic-variables/DynamicVariablesSurface.tsx`）。
const DynamicVariablesSurface = lazy(() =>
  import('@/features/dynamic-variables/DynamicVariablesSurface').then((m) => ({
    default: m.DynamicVariablesSurface,
  })),
);

// 启动失败可能落在任一 surface，故置于两者之上，而非嵌在某个 shell 里。
const BootstrapErrorBanner = lazy(() =>
  import('@/features/top-bar/BootstrapErrorBanner').then((m) => ({
    default: m.BootstrapErrorBanner,
  })),
);

// Portal 渲染，由 `newItem.open` 驱动，故一个实例服务两个 surface。
// 仅限 `type === 'project'`：模块/集合对话框属项目域，住在 `ProjectTree` 里；
// endpoint 没有对话框。
const CreateProjectDialog = lazy(() =>
  import('@/features/create/CreateProjectDialog').then((m) => ({ default: m.CreateProjectDialog })),
);

/**
 * 全窗口 lazy surface 的占位（workspace 外壳、设置独立宿主）。撑满而非
 * 居中 spinner：塌成 0 高度再跳到满高是布局跳动。
 */
function SurfaceFallback({ label }: { label?: string }) {
  return (
    <div className="flex h-full w-full items-center justify-center text-xs text-muted-foreground">
      <span className="opacity-60">{label ?? '加载工作区…'}</span>
    </div>
  );
}

function App() {
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const theme = useAppStore((s) => s.theme);
  // 两个浮层按「使其可见」的状态挂载，不无条件渲染。
  //
  // `lazy()` 组件在被 *render* 的瞬间就发 import，所以启动时写
  // `<Suspense><CreateProjectDialog /></Suspense>` 仍会在首屏下载 chunk ——
  // 只是不阻塞。创建对话框拉进 dialog 原语，并经集合对话框的 select 拉进
  // GSAP（~38 kB gzip），欢迎页一个都不需要。挂在 `open` 上让下载发生在
  // 需要它的那次点击。
  const bootstrapError = useAppStore((s) => s.bootstrapError);
  const newItemOpen = useAppStore((s) => s.newItem.open);
  const newItemType = useAppStore((s) => s.newItem.type);
  // 裸引用而非派生对象，selector 规则见 `stores/app-store.ts`。
  const activeTabId = useAppStore((s) => s.activeTabId);

  // 把持久化的主题写到 <html>。store 从 localStorage 同步读初值，
  // 故挂载时跑一次，设置里改主题时再跑。
  useEffect(() => {
    const root = document.documentElement;
    const apply = (dark: boolean) => {
      root.classList.toggle('dark', dark);
    };
    if (theme === 'dark') {
      apply(true);
    } else if (theme === 'light') {
      apply(false);
    } else {
      // 'system' — follow OS preference, then keep in sync with changes.
      const mq = window.matchMedia('(prefers-color-scheme: dark)');
      apply(mq.matches);
      const onChange = (e: MediaQueryListEvent) => apply(e.matches);
      mq.addEventListener('change', onChange);
      return () => mq.removeEventListener('change', onChange);
    }
  }, [theme]);

  // 全局快捷键：Ctrl+Enter 发送、Ctrl+S 保存。
  // Ctrl+K 命令面板在 <TopBar> 内处理，好与搜索框点击共享 open 状态。
  //
  // IME 守卫：组合输入期间（中文拼音等）浏览器照样派发 `keydown`，
  // 不跳过 `e.isComposing` 就会在候选窗吞掉按键时误触发。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.isComposing) return;

      if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
        e.preventDefault();
        // 复用可见的发送按钮触发一次合成发送：直接 click 它
        const btn = document.querySelector<HTMLButtonElement>('[data-testid="send-btn"]');
        btn?.click();
      }
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault();
        const btn = document.querySelector<HTMLButtonElement>('[data-testid="save-btn"]');
        btn?.click();
      }
      // Esc 不再关闭设置抽屉：设置现在是 tab，用 tab 的 X 或切到别的 tab；
      // 无项目宿主上则是「返回」按钮（见 `features/settings/SettingsSurface.tsx`）。
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, []);

  const onWelcome = activeProjectId === '';
  // 设置与动态变量都是全局资产，不需要项目也能触达。在此判断（而非放进
  // `WelcomeScreen`），才能让两个宿主共用一份面板实现。
  const onWelcomeSettings = onWelcome && activeTabId === SETTINGS_TAB_ID;
  const onWelcomeDynamic = onWelcome && activeTabId === DYNAMIC_TAB_ID;

  return (
    <div className="flex h-screen w-screen flex-col bg-background text-foreground">
      {/* store 能渲染真实数据（或已失败）后移除静态 HTML 启动闪屏，
          见 features/welcome/BootSplashGate.tsx。 */}
      <BootSplashGate />
      {/* 跨两个 surface：第 1 层 bootstrap 失败时欢迎页已挂载，没有顶栏可承载它。
          有错才挂载。 */}
      {bootstrapError !== null && (
        <Suspense fallback={null}>
          <BootstrapErrorBanner />
        </Suspense>
      )}
      {onWelcomeSettings ? (
        <Suspense fallback={<SurfaceFallback label="加载设置…" />}>
          <SettingsSurface />
        </Suspense>
      ) : onWelcomeDynamic ? (
        <Suspense fallback={<SurfaceFallback label="加载动态变量…" />}>
          <DynamicVariablesSurface />
        </Suspense>
      ) : onWelcome ? (
        <WelcomeScreen />
      ) : (
        <Suspense fallback={<SurfaceFallback />}>
          <WorkspaceShell />
        </Suspense>
      )}
      {/* Portal 渲染，且是唯一必须跨 surface 存活的创建对话框：
          项目可从欢迎页、顶栏下拉或命令面板创建，而前两者在不同 surface 上。 */}
      {newItemOpen && newItemType === 'project' && (
        <Suspense fallback={null}>
          <CreateProjectDialog />
        </Suspense>
      )}
    </div>
  );
}

export default App;
