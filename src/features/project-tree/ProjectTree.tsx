import { useMemo, useState } from 'react';
import { Plus, Search } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { TreeContextMenu, type ContextMenuItem } from './TreeContextMenu';
import { ConfirmDialog } from './ConfirmDialog';
import type { EditTarget, TargetKind } from './tree-types';
import { buildTree, countModuleEndpoints, countSubtreeCollections, filterTree } from './tree-utils';
import { useTreeDnd } from './hooks/useTreeDnd';
import { useEditMenu } from './hooks/useEditMenu';
import { useInlineMenus } from './hooks/useInlineMenus';
import { useNewItemFlow } from './hooks/useNewItemFlow';
import { useTreeSelection } from './hooks/useTreeSelection';
import { ModuleRow } from './rows/ModuleRow';
import { TreeChildren } from './rows/TreeChildren';
import { EndpointList } from './rows/EndpointList';
import { CollectionBranch } from './rows/CollectionBranch';

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
  const activeProject = useAppStore((s) => s.projects.find((p) => p.id === s.activeProjectId));

  // ---------- store actions ----------
  const setActiveEndpoint = useAppStore((s) => s.setActiveEndpoint);
  const moveNodes = useAppStore((s) => s.moveNodes);

  // ---------- local UI state ----------
  const [query, setQuery] = useState('');
  const [editing, setEditing] = useState<EditTarget | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; items: ContextMenuItem[] } | null>(null);

  // ---------- derived ----------
  // Every filter below is a `useMemo` over a raw store reference, not a
  // selector: a selector returning a fresh array re-renders on every
  // store write under Zustand v5's `Object.is` comparison.
  const tree = useMemo(
    () => buildTree(nodes, endpointDetails, activeProjectId),
    [nodes, endpointDetails, activeProjectId],
  );
  const filtered = useMemo(
    () => (query.trim() ? filterTree(tree, query.toLowerCase(), endpointDetails) : tree),
    [tree, query, endpointDetails],
  );
  // `rootId` is denormalised onto every node, so "belongs to the open
  // project" is one filter — no walk up to a module to find a project id.
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
  const { openNewItemWithIntent, handleWorkspaceNew } = useNewItemFlow(setEditing);

  // ---------- inline "+" menus ----------
  const { openModulePlusMenu, openCollectionPlusMenu } = useInlineMenus({
    nodes: projectNodes,
    openNewItemWithIntent,
    setMenu,
  });

  // ---------- edit / context menu / delete ----------
  const { toDelete, cancelDelete, startEdit, cancelEdit, saveEdit, openMenu, handleDeleteConfirm } =
    useEditMenu({
      setEditing,
      openNewItemWithIntent,
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
      className="relative flex h-full w-64 shrink-0 flex-col border-r border-border bg-card/30"
      onDragEnd={handleDragEnd}
    >
      {/* Header — the project is named here rather than rendered as a row.
          The project is *not* a node in this tree (it is entered from
          the welcome picker or the top-bar selector), so the panel still
          needs to say which project's modules it is listing. */}
      <div className="flex h-9 shrink-0 items-center justify-between border-b border-border bg-card/40 px-3">
        {activeProject ? (
          <span className="flex min-w-0 items-center gap-1.5">
            <span
              className="h-2 w-2 shrink-0 rounded-sm"
              style={{ backgroundColor: activeProject.color }}
            />
            <span className="truncate text-xs font-medium" title={activeProject.name}>
              {activeProject.name}
            </span>
          </span>
        ) : (
          <span className="label-caps">工作区</span>
        )}
        <Tooltip content="新建（项目 / 模块 / 集合 / 接口）" side="bottom">
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            onClick={handleWorkspaceNew}
            aria-label="新建"
            data-testid="tree-new-btn"
          >
            <Plus className="h-3.5 w-3.5" />
          </Button>
        </Tooltip>
      </div>

      {/* Search */}
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

      {/* Tree — the active project's nodes only. The project itself is
          never a row here; see the header note above. */}
      <div className="flex-1 overflow-y-auto px-1.5 py-1.5">
        {!activeProject ? (
          <div className="px-2 py-6 text-center text-xs text-muted-foreground">
            尚未选择项目。请在欢迎页或顶部项目选择器中选择一个。
          </div>
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
            {/* Top-level create. The project used to be a row in this
                tree and its "+" owned "add a module to this project".
                With the project gone the entry point has to live in
                the tree's own space, and it sits above the modules
                because a module *is* a top-level node here — the same
                position the project row used to occupy.

                Hidden while a filter is active: a module created under
                a query would not match it, so the control would appear
                to do nothing. The header "+" still reaches the same
                modal. */}
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
                  collectionCount={modBranch.collections.length}
                  endpointCount={countModuleEndpoints(modBranch)}
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
                  onDragOver={makeDragOverHandler(
                    { kind: 'module', id: modBranch.module.id },
                    null,
                  )}
                  onDragLeave={makeDragLeaveHandler({ kind: 'module', id: modBranch.module.id })}
                  onDrop={makeDropHandler({ kind: 'module', id: modBranch.module.id }, null)}
                  dropHint={
                    dropHint && dropHint.key === `module:${modBranch.module.id}`
                      ? dropHint.zone
                      : null
                  }
                />
                <TreeChildren expanded={!!modBranch.module.expanded}>
                  {/* Endpoints sitting on the module itself, above its
                  collections — a module accepts both child kinds. */}
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

      {/* Footer */}
      <div className="border-t border-border px-3 py-1.5 text-[10px] text-muted-foreground">
        {tree.length} 个模块 ·{' '}
        {tree.reduce(
          (a, m) => a + m.collections.reduce((b, c) => b + 1 + countSubtreeCollections(c), 0),
          0,
        )}{' '}
        个集合 · {tree.reduce((a, m) => a + countModuleEndpoints(m), 0)} 个接口
        {selectedEndpointIds.size > 1 && (
          <span className="ml-1 text-primary">· 已选 {selectedEndpointIds.size} 个接口</span>
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
    </aside>
  );
}
