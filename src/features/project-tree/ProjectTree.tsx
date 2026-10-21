import { Suspense, lazy, useCallback, useMemo, useState } from 'react';
import { Plus, Search } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { cn, focusRing } from '@/lib/utils';
import { TreeContextMenu, type ContextMenuItem } from './TreeContextMenu';
import { ConfirmDialog } from './ConfirmDialog';
import type { EditTarget, TargetKind } from './tree-types';
import { buildTree, filterTree } from './tree-utils';
import { useTreeDnd } from './hooks/useTreeDnd';
import { useEditMenu } from './hooks/useEditMenu';
import { useInlineMenus } from './hooks/useInlineMenus';
import { useNewItemFlow } from './hooks/useNewItemFlow';
import { useTreeSelection } from './hooks/useTreeSelection';
import { useTransfer } from '@/features/interchange/useTransfer';
import { ModuleRow } from './rows/ModuleRow';
import { TreeChildren } from './rows/TreeChildren';
import { EndpointList } from './rows/EndpointList';
import { CollectionBranch } from './rows/CollectionBranch';

// 项目内两类实体的创建对话框。两者既 `lazy()` **又**挂在 store 的 open 标志上：
// `lazy()` 组件一被渲染就触发 import，无条件挂载会在打开项目时就把对话框基础组件
// （以及经集合对话框的 select 牵进来的 GSAP）拉进 workspace chunk，而不是等到需要它的
// 那次点击。接口没有对话框 —— 见 `useNewItemFlow::createEndpoint`。
const CreateModuleDialog = lazy(() =>
  import('@/features/create/CreateModuleDialog').then((m) => ({ default: m.CreateModuleDialog })),
);
const CreateCollectionDialog = lazy(() =>
  import('@/features/create/CreateCollectionDialog').then((m) => ({
    default: m.CreateCollectionDialog,
  })),
);

// 导入 / 导出弹窗同理：勾选树可能有几百行，不该在打开项目时就进包。
const TransferDialogLazy = lazy(() =>
  import('@/features/interchange/TransferDialog-lazy').then((m) => ({
    default: m.TransferDialogLazy,
  })),
);

// 集合的脚本对话框：它牵着 CodeMirror（应用最大的依赖），故同样懒加载。
const CollectionScriptDialog = lazy(() =>
  import('@/features/scripts/CollectionScriptDialog').then((m) => ({
    default: m.CollectionScriptDialog,
  })),
);

const kindLabel: Record<TargetKind, string> = {
  project: '项目',
  module: '模块',
  collection: '集合',
  endpoint: '接口',
};

export function ProjectTree() {
  // ---------- raw state (all raw references — safe for selectors) ----------
  const nodes = useAppStore((s) => s.nodes);
  const endpointDetails = useAppStore((s) => s.endpointDetails);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const activeEndpointId = useAppStore((s) => s.activeEndpointId);
  // bootstrap 的第二层。`nodes: []` 是歧义的 —— 既可能是「该项目没有模块」，也可能是
  // 「树还没到」，读成前者会让加载途中进入项目的人看到一个笃定但错误的空态。
  // 见 `stores/bootstrap-slice.ts`。
  const nodesLoaded = useAppStore((s) => s.nodesLoaded);
  const nodesError = useAppStore((s) => s.nodesError);
  const activeProject = useAppStore((s) => s.projects.find((p) => p.id === s.activeProjectId));

  // ---------- store actions ----------
  const setActiveEndpoint = useAppStore((s) => s.setActiveEndpoint);
  const moveNodes = useAppStore((s) => s.moveNodes);
  const openTab = useAppStore((s) => s.openTab);
  /**
   * 项目标签是否在前台？刻意写成把整个答案折叠成 **boolean** 的 selector：返回
   * `s.openTabs.find(...)` 的 selector 每次调用都会交出新的引用，导致每次 store 写入都
   * 重渲染整棵树。
   */
  const projectTabActive = useAppStore((s) => {
    if (s.activeTabId !== s.activeProjectId) return false;
    return s.openTabs.some((t) => t.kind === 'project' && t.id === s.activeProjectId);
  });

  // ---------- local UI state ----------
  const [query, setQuery] = useState('');
  const [editing, setEditing] = useState<EditTarget | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; items: ContextMenuItem[] } | null>(null);

  // ---------- derived ----------
  // 下面每个过滤都是对原始 store 引用做的 `useMemo`，而不是 selector：返回新数组的
  // selector 在 Zustand v5 的 `Object.is` 比较下会让每次 store 写入都触发重渲染。
  const tree = useMemo(
    () => buildTree(nodes, endpointDetails, activeProjectId),
    [nodes, endpointDetails, activeProjectId],
  );
  const filtered = useMemo(
    () => (query.trim() ? filterTree(tree, query.toLowerCase(), endpointDetails) : tree),
    [tree, query, endpointDetails],
  );
  // `rootId` 反规范化地存在于每个节点上，故「属于当前打开的项目」只是一次过滤 ——
  // 不必向上走到模块再去找项目 id。
  const projectNodes = useMemo(
    () => nodes.filter((n) => n.rootId === activeProjectId),
    [nodes, activeProjectId],
  );

  // ---------- click / selection handlers ----------
  const {
    openModuleTab,
    openCollectionTab,
    openEndpointTab,
    selectedEndpointIds,
    handleEndpointClick,
  } = useTreeSelection();

  // ---------- new-item flow ----------
  const { openNewItemWithIntent, createEndpoint } = useNewItemFlow(setEditing);

  // 两个对话框的挂载按草稿类型加门，使每个 chunk 绑定到真正需要它的入口：从顶栏打开
  // *项目* 对话框不应顺带拉取模块对话框。
  const newItemOpen = useAppStore((s) => s.newItem.open);
  const newItemType = useAppStore((s) => s.newItem.type);

  // ---------- 导入 / 导出 ----------
  // 弹窗状态住在 hook 里而非 store：只有本组件能打开它，不存在第二个入口要读它的状态。
  const transfer = useTransfer({
    onImported: async (moduleId) => {
      await useAppStore.getState().reloadWorkspace();
      // 展开落点模块：新集合挂在它下面，父级折叠着的话用户看不到自己刚导入了什么。
      // 新建的集合本身保持折叠 —— 一次导入几十个分组，全展开等于把树推到看不见底。
      useAppStore.setState((s) => ({
        nodes: s.nodes.map((n) => (n.id === moduleId ? { ...n, expanded: true } : n)),
      }));
    },
  });

  const { session: transferSession, openImport, openExport } = transfer;

  const startTransfer = useCallback(
    (mode: 'import' | 'export', module: { id: string; name: string }) => {
      const session = {
        projectId: activeProjectId,
        moduleId: module.id,
        moduleName: module.name,
      };
      if (mode === 'import') openImport(session);
      else void openExport(session);
    },
    [activeProjectId, openImport, openExport],
  );

  // ---------- inline "+" menus ----------
  const { openModulePlusMenu, openCollectionPlusMenu } = useInlineMenus({
    nodes: projectNodes,
    openNewItemWithIntent,
    createEndpointAt: createEndpoint,
    startTransfer,
    setMenu,
  });

  // ---------- edit / context menu / delete ----------
  const {
    toDelete,
    cancelDelete,
    startEdit,
    cancelEdit,
    saveEdit,
    openMenu,
    handleDeleteConfirm,
    scriptCollection,
    closeCollectionScripts,
  } = useEditMenu({
    setEditing,
    openNewItemWithIntent,
    createEndpointAt: createEndpoint,
    openEndpointTab,
    setMenu,
  });

  // ---------- drag-and-drop helpers ----------
  const {
    dropHint,
    setDataTransfer,
    makeDropHandler,
    makeDragOverHandler,
    makeDragLeaveHandler,
    handleDragEnd,
  } = useTreeDnd({
    nodes: projectNodes,
    activeProjectId,
    moveNodes,
  });

  // ---------- render ----------
  return (
    <aside
      // 用实色 `--card` 而非 `bg-card/30`：半透明叠加正是表面色阶要替掉的手搓透明；
      // `--card` 现在是区别于 `--background` 的一级，面板读起来是一层而非一层薄雾，
      // 其内部状态交给 `--overlay-hover` / `--surface-select`。
      className="relative flex h-full w-64 shrink-0 flex-col border-r border-border bg-card"
      onDragEnd={handleDragEnd}
    >
      {/* 表头 —— 项目在这里被点名，而不是渲染成一行。项目**不是**本树中的节点（它从
          欢迎页选择器或顶栏选择器进入），故面板仍需说明自己列的是哪个项目的模块。

          名字是**按钮**：它打开（或重新激活）项目标签，标题因此成为进入项目自身工作区
          的入口而非一个标签。项目不在树里，没有别的行能表示「给我看这个项目」，故关掉
          它的标签后面板就没有回到项目卡片的路径。

          此处刻意没有「+」。项目内部唯一能创建的是模块 —— 集合和接口都需要父级，而项目
          不归这棵树管。下面列表已独占那一个动作（「新建模块」），表头按钮既是通往单个选项
          的第二条路径，又顺带提供三个在这里根本用不了的选项。项目在欢迎页的「新建项目」
          卡片或顶栏选择器里创建。 */}
      <div className="flex h-9 shrink-0 items-center border-b border-border bg-card/40 px-3">
        {activeProject ? (
          // 标题就是项目标签自己的入口。项目不是这棵树里的一行，回项目卡片的路只有标签条
          // —— 关掉那个标签（或标签条已满）的用户就点不到任何东西。
          //
          // `openTab` 是激活已有标签而非复制一个，故项目标签已开着时再点也安全；
          // `projectTabActive` 只驱动「你在这里」的填充。
          <button
            type="button"
            onClick={() => openTab('project', activeProject.id)}
            title="打开项目标签"
            aria-label={`打开项目标签 ${activeProject.name}`}
            aria-current={projectTabActive ? 'true' : undefined}
            className={cn(
              'flex min-w-0 flex-1 items-center gap-1.5 rounded px-1 py-0.5 text-left transition-colors',
              focusRing,
              projectTabActive
                ? 'bg-surface-select text-foreground'
                : 'text-muted-foreground hover:bg-overlay-hover hover:text-foreground',
            )}
          >
            <span
              className="h-2 w-2 shrink-0 rounded-sm"
              style={{ backgroundColor: activeProject.color }}
            />
            <span className="truncate text-xs font-medium" title={activeProject.name}>
              {activeProject.name}
            </span>
          </button>
        ) : (
          <span className="label-caps">工作区</span>
        )}
      </div>

      {/* 搜索 */}
      <div className="px-2 pt-2">
        <div className="relative">
          <Search className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="过滤…"
            className="h-7 pl-7 text-xs"
          />
        </div>
      </div>

      {/* 树 —— 只含当前项目的节点。项目本身在此永远不是一行；见上方表头说明。 */}
      <div className="flex-1 overflow-y-auto px-1.5 py-1.5">
        {!activeProject ? (
          <div className="px-2 py-6 text-center text-xs text-muted-foreground">
            尚未选择项目。请在欢迎页或顶部项目选择器中选择一个。
          </div>
        ) : nodesError ? (
          <div className="space-y-2 px-2 py-6 text-center">
            <p className="text-xs text-destructive">项目内容加载失败</p>
            <p className="text-[11px] text-muted-foreground" title={nodesError}>
              {nodesError}
            </p>
          </div>
        ) : !nodesLoaded ? (
          <div className="px-2 py-6 text-center text-xs text-muted-foreground">加载项目内容…</div>
        ) : query.trim() && filtered.length === 0 ? (
          <div className="px-2 py-6 text-center text-xs text-muted-foreground">
            没有匹配「{query}」的节点。
          </div>
        ) : filtered.length === 0 ? (
          <div className="space-y-2 px-2 py-6 text-center">
            <p className="text-xs text-muted-foreground">该项目还没有模块。</p>
            <Button
              variant="outline"
              size="sm"
              onClick={() => openNewItemWithIntent({ type: 'module' })}
              className="h-7 text-xs"
            >
              <Plus className="h-3.5 w-3.5" />
              新建模块
            </Button>
          </div>
        ) : (
          <>
            {/* 顶层新建。项目不是这棵树里的一行，「给本项目加模块」这个入口必须落在树
                自己的空间里；它置于模块之上，因为模块*就是*这里的顶层节点。

                这是项目内添加模块的**唯一**途径（表头的「+」已去掉，见表头说明）。过滤
                生效时隐藏 —— 查询下创建的模块不会匹配它，这个控件看起来就会是无效的。 */}
            {!query.trim() && (
              <button
                type="button"
                onClick={() => openNewItemWithIntent({ type: 'module' })}
                className="mb-0.5 flex w-full items-center gap-1.5 rounded-md px-2 py-1 text-left text-xs text-muted-foreground transition-colors hover:bg-accent/60 hover:text-foreground"
                data-testid="tree-add-module-btn"
              >
                <Plus className="h-3 w-3 shrink-0" />
                新建模块
              </button>
            )}
            {filtered.map((modBranch) => (
              <div key={modBranch.module.id} className="mb-0.5">
                <ModuleRow
                  module={modBranch.module}
                  isEditing={editing?.kind === 'module' && editing.id === modBranch.module.id}
                  onSelect={() => openModuleTab(modBranch.module.id)}
                  onStartEdit={() => startEdit('module', modBranch.module.id)}
                  onSaveEdit={(name) => saveEdit('module', modBranch.module.id, name)}
                  onCancelEdit={cancelEdit}
                  onContextMenu={(e) => openMenu(e, 'module', modBranch.module.id)}
                  onPlusClick={openModulePlusMenu(modBranch.module.id)}
                  onDragStart={(e) => {
                    setDataTransfer(e, { kind: 'module', ids: [modBranch.module.id] });
                  }}
                  onDragEnd={handleDragEnd}
                  onDragOver={makeDragOverHandler({
                    kind: 'module',
                    id: modBranch.module.id,
                  })}
                  onDragLeave={makeDragLeaveHandler({ kind: 'module', id: modBranch.module.id })}
                  onDrop={makeDropHandler({ kind: 'module', id: modBranch.module.id })}
                  dropHint={
                    dropHint && dropHint.key === `module:${modBranch.module.id}`
                      ? dropHint.zone
                      : null
                  }
                />
                <TreeChildren expanded={!!modBranch.module.expanded}>
                  {/* 直接挂在模块上的接口，排在其集合之上 —— 模块同时接受两种子节点。 */}
                  {modBranch.endpoints.length > 0 && (
                    <EndpointList
                      endpoints={modBranch.endpoints}
                      details={modBranch.endpointDetails}
                      editing={editing}
                      selectedEndpointIds={selectedEndpointIds}
                      activeEndpointId={activeEndpointId}
                      onEndpointClick={handleEndpointClick}
                      onEndpointStartEdit={(id) => {
                        setActiveEndpoint(id);
                        startEdit('endpoint', id);
                      }}
                      onEndpointContextMenu={(e, id) => {
                        setActiveEndpoint(id);
                        openMenu(e, 'endpoint', id);
                      }}
                      onEndpointSaveEdit={(id, name) => saveEdit('endpoint', id, name)}
                      onCancelEdit={cancelEdit}
                      dragContext={{
                        moveNodes,
                        setDataTransfer,
                        makeDragOverHandler,
                        makeDragLeaveHandler,
                        makeDropHandler,
                        handleDragEnd,
                        dropHint,
                      }}
                    />
                  )}
                  <CollectionBranch
                    branches={modBranch.collections}
                    depth={1}
                    editing={editing}
                    onSelectCollection={openCollectionTab}
                    onStartEditCollection={(id) => startEdit('collection', id)}
                    onSaveEditCollection={(id, name) => saveEdit('collection', id, name)}
                    onCancelEdit={cancelEdit}
                    onContextMenu={openMenu}
                    onPlusClick={openCollectionPlusMenu}
                    selectedEndpointIds={selectedEndpointIds}
                    activeEndpointId={activeEndpointId}
                    onEndpointClick={handleEndpointClick}
                    onEndpointStartEdit={(id) => {
                      setActiveEndpoint(id);
                      startEdit('endpoint', id);
                    }}
                    onEndpointContextMenu={(e, id) => {
                      setActiveEndpoint(id);
                      openMenu(e, 'endpoint', id);
                    }}
                    onEndpointSaveEdit={(id, name) => saveEdit('endpoint', id, name)}
                    dragContext={{
                      moveNodes,
                      setDataTransfer,
                      makeDragOverHandler,
                      makeDragLeaveHandler,
                      makeDropHandler,
                      handleDragEnd,
                      dropHint,
                    }}
                  />
                </TreeChildren>
              </div>
            ))}
          </>
        )}
      </div>

      {/* ---- Overlays ---- */}
      {menu && (
        <TreeContextMenu
          open
          x={menu.x}
          y={menu.y}
          items={menu.items}
          onClose={() => setMenu(null)}
        />
      )}
      {toDelete && (
        <ConfirmDialog
          open
          onOpenChange={(o) => !o && cancelDelete()}
          title={`删除${kindLabel[toDelete.kind]}「${toDelete.name}」？`}
          message={
            toDelete.cascade ? (
              <span>此操作不可撤销，{toDelete.cascade}。</span>
            ) : (
              '此操作不可撤销。'
            )
          }
          destructive
          confirmLabel="删除"
          onConfirm={handleDeleteConfirm}
        />
      )}
      {/* 集合的脚本对话框 —— 同样只在打开时挂载，编辑器那块的 CodeMirror 不进常驻 chunk。 */}
      {scriptCollection && (
        <Suspense fallback={null}>
          <CollectionScriptDialog
            open
            onOpenChange={(o) => !o && closeCollectionScripts()}
            nodeId={scriptCollection.id}
            nodeName={scriptCollection.name}
          />
        </Suspense>
      )}

      {/* ---- 创建对话框，每类一个 ----
          各自只在自己的草稿打开时挂载，故两者不会共存，每个 chunk 都在需要它的那次点击
          才加载。 */}
      {newItemOpen && newItemType === 'module' && (
        <Suspense fallback={null}>
          <CreateModuleDialog />
        </Suspense>
      )}
      {newItemOpen && newItemType === 'collection' && (
        <Suspense fallback={null}>
          <CreateCollectionDialog />
        </Suspense>
      )}
      {/* 导入 / 导出。挂在 `session` 上而非常驻：这块面板可能几百行，而绝大多数会话
          从不打开它。 */}
      {transferSession && (
        <Suspense fallback={null}>
          <TransferDialogLazy controller={transfer} />
        </Suspense>
      )}
    </aside>
  );
}
