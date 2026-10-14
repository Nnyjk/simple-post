/**
 * WorkspaceShell —— 项目激活时的应用框架。
 *
 * 为什么单列一个模块
 * -----------------------------
 * `App.tsx` 按 `activeProjectId === ''` 在两个界面间切换：独立的欢迎页，与本组件。把工作区
 * 放在自己的模块里，才让 `App.tsx` 能 `lazy()` 它 —— 于是打开应用只取 store 与项目选择器。
 * 树、顶栏、HTTP tester、设置面板与 CodeMirror 编辑器，都要等用户真的进入项目才取。
 *
 * 这与 `bootstrap-slice.ts` 里的两层 `bootstrap()` 配套：第一层（`projects`）放行启动闪屏，
 * 第二层（树 / 详情 / 环境）在其后就位。
 *
 * 这里刻意不挂载什么
 * --------------------------------------
 * `ProjectTree` 只在项目激活时渲染。没有项目时它只有「尚未选择项目」一列占位 —— 一条死
 * 掉的 256px 面板，外加一个搜索框、一个没有父级项目就必然失败的「新建」按钮，以及一个
 * 恒为零的页脚计数。
 *
 * 没有项目时也要能到设置面板这件事由 `App.tsx` 负责（
 * `features/settings/SettingsSurface.tsx`），本外壳因此不必在没有树的情况下渲染设置。两个
 * 宿主挂载的是同一个 `SettingsPanel`，只有外围 chrome 不同。
 */

import { Suspense, lazy, useEffect, useRef, useState } from 'react';
import { TopBar } from '@/features/top-bar/TopBar';
import { ProjectTree } from '@/features/project-tree/ProjectTree';
import { HttpTester } from '@/features/http-tester/HttpTester';
import { WorkspaceTabs } from '@/features/workspace/WorkspaceTabs';
import { Splitter } from '@/components/ui/splitter';
import { useAppStore } from '@/stores/app-store';

// ---------------------------------------------------------------------------
// 代码分割 —— 每个 lazy 边界的原因见其上方的说明。
//
// 这些都是只存在于*本外壳之内*的面板，故把它们异步化在首屏毫无代价（本外壳本身就已是
// 异步 chunk），还让它们互不落进对方的 chunk。
// ---------------------------------------------------------------------------

// CodeMirror（未压缩 425 kB / gzip 139 kB）。只有接口标签需要它。
const ResponseViewer = lazy(() =>
  import('@/features/response-viewer/ResponseViewer').then((m) => ({ default: m.ResponseViewer })),
);

// 设置是标签后面的一整块面板；启动时永远用不到。
const SettingsPanel = lazy(() =>
  import('@/features/settings/SettingsPanel').then((m) => ({ default: m.SettingsPanel })),
);
const EntitySettings = lazy(() =>
  import('@/features/workspace/EntitySettings').then((m) => ({ default: m.EntitySettings })),
);

// 对话框：在树里挂载，但只对用户动作有反应，故启动时纯属死重。
//
// `NewItemModal` 与 `BootstrapErrorBanner` **不**在这里 —— 两者从欢迎页也触达得到，故由
// `App.tsx` 在界面切换之上各持一份。重复挂载会为同一份 store 状态挂两个 portal。
const DirtyCloseDialog = lazy(() =>
  import('@/features/workspace/DirtyCloseDialog').then((m) => ({ default: m.DirtyCloseDialog })),
);

/**
 * 尚未到达的 chunk 的占位。
 *
 * 尺寸填满自己的槽位而非一个居中的 spinner：先塌成 spinner 再猛地撑满高度的 lazy 面板
 * 就是一次布局跳动 —— 而这正是整套拆分要消除的卡顿。
 */
function PanelFallback({ label }: { label?: string }) {
  return (
    <div
      className="flex min-h-0 flex-1 items-center justify-center text-xs text-muted-foreground"
      aria-busy="true"
    >
      {label ?? <span className="opacity-60">加载中…</span>}
    </div>
  );
}

export function WorkspaceShell() {
  const activeTab = useAppStore((s) => s.openTabs.find((t) => t.id === s.activeTabId) ?? null);
  const activeProjectId = useAppStore((s) => s.activeProjectId);

  // 本地的分隔条状态 —— HttpTester（上）/ ResponseViewer（下）的比例。仅在当前标签是接口
  // 时有意义；项目 / 模块 / 集合标签的内容区是单一滚动区。
  const [basis, setBasis] = useState<string>('50%');
  const containerRef = useRef<HTMLDivElement>(null);
  const resetBasis = () => setBasis('50%');

  // 在 bootstrap 窗口内预热编辑器 chunk。
  //
  // 本外壳本身是 lazy，所以它挂载时模块图已在解析中；发起与 lazy 组件相同的动态 import
  // 会填好注册表，等 store 有接口标签要渲染时，CodeMirror chunk 已解码完毕，`lazy()`
  // 边界同步解析。同一 specifier → 同一 chunk，故这只是把迟早会发生的这一次取件提前、
  // 并挪出关键路径。
  //
  // 以「当前标签是接口」为门。无条件预热会把约 450 kB 编辑器拉进一个只显示项目卡片的工作区。
  useEffect(() => {
    if (activeTab?.kind !== 'endpoint') return;
    const t = setTimeout(() => {
      void import('@/features/http-tester/BodyEditor');
      void import('@/features/response-viewer/ResponseViewer');
      void import('@/features/http-tester/DocsEditor');
    }, 0);
    return () => clearTimeout(t);
  }, [activeTab?.kind]);

  // 按当前标签类型决定右栏渲染什么。
  //   endpoint → 请求 / 响应上下分栏
  //   project / module / collection → EntitySettings：该实体自己的单列面板（基础信息，
  //     再是该类型所属的配置 —— 项目的环境、模块按环境的 baseUrls）。面板不点名实体，
  //     上方的标签才点。
  //   settings → 设置面板（本身就是一个标签，不是抽屉）
  //
  // 「有项目但没有激活标签」正是关掉最后一个标签后的状态：项目本身就是自然的落地面，
  // 项目重命名 / 删除现在也住在那里，因为项目不再是树中的一行。「无项目且无标签」则没
  // 东西可显示 —— 但那种状态由 `App.tsx` 渲染欢迎页，故永远不会走到这里。
  const renderActiveContent = () => {
    if (!activeTab) {
      return (
        <Suspense fallback={<PanelFallback />}>
          <EntitySettings kind="project" entityId={activeProjectId} />
        </Suspense>
      );
    }
    // 草稿接口渲染与已保存接口同一个 tester。两者刻意不是两个组件：
    // `useActiveEndpointResource` 以同一种形状交出两者，故 tester 只跑一条代码路径，未保存
    // 的接口也能用与已保存接口相同的 UI 编辑、发送与预览。
    if (activeTab.kind === 'endpoint' || activeTab.kind === 'endpoint-draft') {
      return (
        <>
          <div
            style={{ flexBasis: basis, flexGrow: 0, flexShrink: 0 }}
            className="flex min-h-0 flex-col"
          >
            <HttpTester />
          </div>
          <Splitter
            direction="horizontal"
            containerRef={containerRef}
            basis={basis}
            onResize={setBasis}
            onReset={resetBasis}
            minSize={240}
            minOtherSize={160}
          />
          <div className="min-h-0 flex-1">
            <Suspense fallback={<PanelFallback />}>
              <ResponseViewer />
            </Suspense>
          </div>
        </>
      );
    }
    if (activeTab.kind === 'settings') {
      return (
        <Suspense fallback={<PanelFallback />}>
          <SettingsPanel />
        </Suspense>
      );
    }
    return (
      <Suspense fallback={<PanelFallback />}>
        <EntitySettings kind={activeTab.kind} entityId={activeTab.id} />
      </Suspense>
    );
  };

  return (
    <div className="flex h-full min-h-0 w-full flex-col">
      <TopBar />
      <main className="flex min-h-0 flex-1">
        {/* 没有项目 → 没有树。占位列为何还不如什么都没有，见模块头部的说明。 */}
        {activeProjectId !== '' && <ProjectTree />}
        <div ref={containerRef} className="flex min-w-0 flex-1 flex-col">
          <WorkspaceTabs />
          {renderActiveContent()}
        </div>
      </main>
      <Suspense fallback={null}>
        <DirtyCloseDialog />
      </Suspense>
    </div>
  );
}
