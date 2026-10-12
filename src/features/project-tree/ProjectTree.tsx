import { Suspense, lazy, useMemo, useState } from 'react';
import { Plus, Search } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { TreeContextMenu, type ContextMenuItem } from './TreeContextMenu';
import { ConfirmDialog } from './ConfirmDialog';
import type { EditTarget, TargetKind } from './tree-types';
import { buildTree, countModuleEndpoints, filterTree } from './tree-utils';
import { useTreeDnd } from './hooks/useTreeDnd';
import { useEditMenu } from './hooks/useEditMenu';
import { useInlineMenus } from './hooks/useInlineMenus';
import { useNewItemFlow } from './hooks/useNewItemFlow';
import { useTreeSelection } from './hooks/useTreeSelection';
import { ModuleRow } from './rows/ModuleRow';
import { TreeChildren } from './rows/TreeChildren';
import { EndpointList } from './rows/EndpointList';
import { CollectionBranch } from './rows/CollectionBranch';

// The create dialogs for the two kinds that live in a project. Both are
// `lazy()` *and* mounted on the store's open flag: a `lazy()` component
// fires its import the moment it is rendered, so an unconditional mount
// would pull the dialog primitive (and, through the collection dialog's
// select, GSAP) into the workspace chunk at project-open time instead of
// on the click that needs it. An endpoint has no dialog — see
// `useNewItemFlow::createEndpoint`.
const CreateModuleDialog = lazy(() =>
  import('@/features/create/CreateModuleDialog').then((m) => ({ default: m.CreateModuleDialog })),
);
const CreateCollectionDialog = lazy(() =>
  import('@/features/create/CreateCollectionDialog').then((m) => ({
    default: m.CreateCollectionDialog,
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
  // Layer 2 of the bootstrap. `nodes: []` is ambiguous — it means both
  // "this project has no modules" and "the tree has not arrived yet" —
  // and reading it as the first would show a confident, wrong empty state
  // to anyone who enters a project while the load is still in flight.
  // See `stores/bootstrap-slice.ts`.
  const nodesLoaded = useAppStore((s) => s.nodesLoaded);
  const nodesError = useAppStore((s) => s.nodesError);
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
  const { openNewItemWithIntent, createEndpoint } = useNewItemFlow(setEditing);

  // Gating the two dialog mounts on the draft's kind keeps each chunk
  // tied to the entry point that needs it: opening the *project* dialog
  // from the top bar must not fetch the module dialog.
  const newItemOpen = useAppStore((s) => s.newItem.open);
  const newItemType = useAppStore((s) => s.newItem.type);

  // ---------- inline "+" menus ----------
  const { openModulePlusMenu, openCollectionPlusMenu } = useInlineMenus({
    nodes: projectNodes,
    openNewItemWithIntent,
    createEndpointAt: createEndpoint,
    setMenu,
  });

  // ---------- edit / context menu / delete ----------
  const { toDelete, cancelDelete, startEdit, cancelEdit, saveEdit, openMenu, handleDeleteConfirm } =
    useEditMenu({
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
      className="relative flex h-full w-64 shrink-0 flex-col border-r border-border bg-card/30"
      onDragEnd={handleDragEnd}
    >
      {/* Header — the project is named here rather than rendered as a row.
          The project is *not* a node in this tree (it is entered from
          the welcome picker or the top-bar selector), so the panel still
          needs to say which project's modules it is listing.

          There is deliberately no "+" next to this title. The header
          used to open a menu offering 项目 / 模块 / 集合 / 接口, but the
          only thing creatable from inside a project is a module — the
          other two need a parent, and a project is not this tree's
          business. The list below owns that one action ("新建模块"), so
          the button was a second path to a single option that also
          offered three that cannot work here. Projects are created from
          the welcome screen's 新建项目 card or the top-bar selector. */}
      <div className="flex h-9 shrink-0 items-center border-b border-border bg-card/40 px-3">
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
            {/* Top-level create. The project used to be a row in this
                tree and its "+" owned "add a module to this project".
                With the project gone the entry point has to live in
                the tree's own space, and it sits above the modules
                because a module *is* a top-level node here — the same
                position the project row used to occupy.

                This is now the ONLY way to add a module from inside a
                project: the header's "+" is gone (see the header note).
                Hidden while a filter is active — a module created under
                a query would not match it, so the control would appear
                to do nothing. */}
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

      {/* ---- Create dialogs, one per kind ----
          Each is mounted only while its own draft is open, so the two
          never coexist and each chunk loads on the click that needs it. */}
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
    </aside>
  );
}
