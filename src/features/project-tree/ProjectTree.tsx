import { useMemo, useState } from 'react';
import { Plus, Search } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { TreeContextMenu, type ContextMenuItem } from './TreeContextMenu';
import { ConfirmDialog } from './ConfirmDialog';
import type { EditTarget, TargetKind } from './tree-types';
import {
  buildTree,
  countSubtreeCollections,
  countSubtreeEndpoints,
  countTreeEndpoints,
  filterTree,
} from './tree-utils';
import { useTreeDnd } from './hooks/useTreeDnd';
import { useEditMenu } from './hooks/useEditMenu';
import { useInlineMenus } from './hooks/useInlineMenus';
import { useNewItemFlow } from './hooks/useNewItemFlow';
import { useTreeSelection } from './hooks/useTreeSelection';
import { ProjectRow } from './rows/ProjectRow';
import { ModuleRow } from './rows/ModuleRow';
import { TreeChildren } from './rows/TreeChildren';
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
  const projectModules = useMemo(
    () =>
      projectNodes
        .filter((n) => n.kind === 'module' && n.parentId === null)
        .sort((a, b) => a.sortOrder - b.sortOrder),
    [projectNodes],
  );
  const projectCollections = useMemo(
    () => projectNodes.filter((n) => n.kind === 'collection'),
    [projectNodes],
  );
  const projectEndpoints = useMemo(
    () => projectNodes.filter((n) => n.kind === 'endpoint'),
    [projectNodes],
  );

  // ---------- click / selection handlers ----------
  const {
    openProjectTab,
    openModuleTab,
    openCollectionTab,
    openEndpointTab,
    selectedEndpointIds,
    handleEndpointClick,
  } = useTreeSelection();

  // ---------- new-item flow ----------
  const { openNewItemWithIntent, handleWorkspaceNew } = useNewItemFlow(setEditing);

  // ---------- inline "+" menus ----------
  const { openProjectPlusMenu, openModulePlusMenu, openCollectionPlusMenu } = useInlineMenus({
    nodes: projectNodes,
    projectModules,
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
      {/* Header */}
      <div className="flex h-9 shrink-0 items-center justify-between border-b border-border bg-card/40 px-3">
        <span className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
          工作区
        </span>
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

      {/* Tree */}
      <div className="flex-1 overflow-y-auto px-1.5 py-1.5">
        {!activeProject ? (
          <div className="px-2 py-6 text-center text-xs text-muted-foreground">
            暂无项目。点击右上角 + 新建一个。
          </div>
        ) : (
          <ProjectRow
            project={activeProject}
            moduleCount={projectModules.length}
            collectionCount={projectCollections.length}
            endpointCount={projectEndpoints.length}
            isEditing={editing?.kind === 'project' && editing.id === activeProject.id}
            onSelect={() => openProjectTab(activeProject.id)}
            onStartEdit={() => startEdit('project', activeProject.id)}
            onSaveEdit={(name) => saveEdit('project', activeProject.id, name)}
            onCancelEdit={cancelEdit}
            onContextMenu={(e) => openMenu(e, 'project', activeProject.id)}
            onPlusClick={openProjectPlusMenu}
          />
        )}
        {filtered.map(({ module, collections: modCollections }) => (
          <div key={module.id} className="mb-0.5">
            <ModuleRow
              module={module}
              collectionCount={modCollections.length}
              endpointCount={countTreeEndpoints(modCollections)}
              isEditing={editing?.kind === 'module' && editing.id === module.id}
              onSelect={() => openModuleTab(module.id)}
              onStartEdit={() => startEdit('module', module.id)}
              onSaveEdit={(name) => saveEdit('module', module.id, name)}
              onCancelEdit={cancelEdit}
              onContextMenu={(e) => openMenu(e, 'module', module.id)}
              onPlusClick={openModulePlusMenu(module.id)}
              onDragStart={(e) => {
                setDataTransfer(e, { kind: 'module', ids: [module.id] });
              }}
              onDragEnd={handleDragEnd}
              onDragOver={makeDragOverHandler({ kind: 'module', id: module.id }, null)}
              onDragLeave={makeDragLeaveHandler({ kind: 'module', id: module.id })}
              onDrop={makeDropHandler({ kind: 'module', id: module.id }, null)}
              dropHint={dropHint && dropHint.key === `module:${module.id}` ? dropHint.zone : null}
            />
            <TreeChildren expanded={!!module.expanded}>
              <CollectionBranch
                branches={modCollections}
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
      </div>

      {/* Footer */}
      <div className="border-t border-border px-3 py-1.5 text-[10px] text-muted-foreground">
        {tree.length} 个模块 ·{' '}
        {tree.reduce(
          (a, m) => a + m.collections.reduce((b, c) => b + 1 + countSubtreeCollections(c), 0),
          0,
        )}{' '}
        个集合 ·{' '}
        {tree.reduce(
          (a, m) => a + m.collections.reduce((b, c) => b + countSubtreeEndpoints(c), 0),
          0,
        )}{' '}
        个接口
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
