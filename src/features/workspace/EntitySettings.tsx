/**
 * EntitySettings — config panels for project / module / collection tabs.
 *
 * These three entity kinds have no HTTP request of their own, so the
 * right-pane view is a metadata editor instead of the request tester.
 * A single component renders all three: the kind prop decides which
 * fields are editable and which related entity list to show.
 *
 * Why one component instead of three?
 *   - The visual chrome (header + scrollable body + empty/error states)
 *     is identical.
 *   - Each kind just needs a slightly different field set and list
 *     section. Splitting into three files would duplicate ~80% of the
 *     layout.
 *
 * Persistence: every field writes through the existing store update*
 * actions, so reload + tab strip + duplicate / delete cascades continue
 * to work without any new state plumbing.
 */

import { useEffect, useMemo, useState } from 'react';
import {
  Package,
  FolderOpen,
  FolderClosed,
  Hash,
  Plus,
  Globe,
  Trash2,
  ChevronRight,
  ChevronDown,
  Star,
} from 'lucide-react';
import { useAppStore, type TabKind } from '@/stores/app-store';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { Badge } from '@/components/ui/badge';
import { cn, methodColorVar } from '@/lib/utils';
import { useLocalExpansion } from '@/hooks/useLocalExpansion';
import { Section, Field, EmptyHint, NotFound, ColorRow } from '@/components/ui/section';
import { InlineEdit } from '@/components/ui/InlineEdit';

interface EntitySettingsProps {
  kind: TabKind;
  entityId: string;
}

// ----------------------------------------------------------------
//  public component
// ----------------------------------------------------------------

export function EntitySettings({ kind, entityId }: EntitySettingsProps) {
  // The three settings panels may all have in-flight sub-entity
  // renames. We expose a tiny counter so they can flag the tab as
  // "dirty" while edits are in progress, and the WorkspaceTabs strip
  // can then either show a "•" indicator or refuse to close it
  // through "Close Saved". Without this hook, navigating away
  // silently drops an uncommitted double-click rename.
  const markTabDirty = useAppStore((s) => s.markTabDirty);
  const [editingCount, setEditingCount] = useState(0);
  useEffect(() => {
    markTabDirty(entityId, editingCount > 0);
    // entityId / markTabDirty are stable; we only want to re-fire when
    // the editing count changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editingCount]);

  const editState: EditCounter = useMemo(
    () => ({
      onStart: () => setEditingCount((c) => c + 1),
      onEnd: () => setEditingCount((c) => Math.max(0, c - 1)),
    }),
    [],
  );

  switch (kind) {
    case 'project':
      return <ProjectSettings projectId={entityId} editState={editState} />;
    case 'module':
      return <ModuleSettings moduleId={entityId} editState={editState} />;
    case 'collection':
      return <CollectionSettings collectionId={entityId} editState={editState} />;
    default:
      // Should never happen — endpoint tabs route to HttpTester instead.
      return null;
  }
}

interface EditCounter {
  onStart: () => void;
  onEnd: () => void;
}

// ----------------------------------------------------------------
//  Project
// ----------------------------------------------------------------

function ProjectSettings({ projectId, editState }: { projectId: string; editState: EditCounter }) {
  const project = useAppStore((s) => s.projects.find((p) => p.id === projectId));
  // Read raw state — filtering / sorting happens in the component to
  // satisfy Zustand v5's "do not allocate in selectors" rule.
  const allModules = useAppStore((s) => s.modules);
  const collections = useAppStore((s) => s.collections);
  const endpoints = useAppStore((s) => s.endpoints);
  const allEnvironments = useAppStore((s) => s.environments);

  const modules = useMemo(
    () =>
      allModules
        .filter((m) => m.projectId === projectId)
        .sort((a, b) => a.sortOrder - b.sortOrder),
    [allModules, projectId],
  );
  const environments = useMemo(
    () => allEnvironments.filter((e) => e.projectId === projectId),
    [allEnvironments, projectId],
  );

  const updateProject = useAppStore((s) => s.updateProject);
  const setEnvironmentVar = useAppStore((s) => s.setEnvironmentVar);
  const addEnvironment = useAppStore((s) => s.addEnvironment);
  const updateEnvironment = useAppStore((s) => s.updateEnvironment);
  const setEnvironmentBaseUrl = useAppStore((s) => s.setEnvironmentBaseUrl);
  const deleteEnvironment = useAppStore((s) => s.deleteEnvironment);
  const setEnvironmentActive = useAppStore((s) => s.setEnvironmentActive);
  const setActiveProject = useAppStore((s) => s.setActiveProject);
  const updateModule = useAppStore((s) => s.updateModule);
  const updateCollection = useAppStore((s) => s.updateCollection);
  const addBaseUrlDefinition = useAppStore((s) => s.addBaseUrlDefinition);
  const updateBaseUrlDefinition = useAppStore((s) => s.updateBaseUrlDefinition);
  const deleteBaseUrlDefinition = useAppStore((s) => s.deleteBaseUrlDefinition);
  const setDefaultBaseUrlDefinition = useAppStore((s) => s.setDefaultBaseUrlDefinition);
  const openTab = useAppStore((s) => s.openTab);
  // Local-only expansion so the right pane doesn't drag the left tree
  // along when it expands/collapses. See `useLocalExpansion` for why.
  const moduleExp = useLocalExpansion();
  const collectionExp = useLocalExpansion();
  // baseUrl definition add form — local state to the right pane so
  // the user can type a name before committing. The new definition
  // id is generated by the store and ends up on the project record
  // directly via `addBaseUrlDefinition`.
  const [newDefName, setNewDefName] = useState('');

  if (!project) return <NotFound label="项目" />;

  const projectEnvs = useMemo(
    () => allEnvironments.filter((e) => e.projectId === project.id),
    [allEnvironments, project.id],
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SettingsHeader
        icon={<Package className="h-4 w-4 text-violet-400" />}
        badge="项目"
        title={project.name}
        onTitleChange={(name) => updateProject(project.id, { name })}
        description={project.description}
        onDescriptionChange={(description) => updateProject(project.id, { description })}
        onEditStart={editState.onStart}
        onEditEnd={editState.onEnd}
      />

      <div className="min-h-0 flex-1 overflow-y-auto">
        <Section title="基本信息">
          <Field label="主题色">
            <ColorRow value={project.color} onChange={(c) => updateProject(project.id, { color: c })} />
          </Field>
        </Section>

        <Section
          title="BaseUrl 定义"
          right={
            <span className="text-[10px] text-muted-foreground">
              {project.baseUrlDefinitions.length} 个
            </span>
          }
        >
          {project.baseUrlDefinitions.length === 0 ? (
            <EmptyHint text="该项目还没有 baseUrl 定义。在下方新增一个或多个名称（如「鉴权服务」），然后到环境里给每个名称填具体地址。" />
          ) : (
            <ul className="space-y-1">
              {project.baseUrlDefinitions.map((def) => {
                const isDefault = project.defaultBaseUrlDefinitionId === def.id;
                return (
                  <li
                    key={def.id}
                    className={cn(
                      'flex items-center gap-2 rounded-sm border border-border/60 bg-background/40 px-2 py-1.5',
                      isDefault && 'border-primary/40',
                    )}
                  >
                    <Globe className="h-3.5 w-3.5 shrink-0 text-primary/80" />
                    <InlineEdit
                      value={def.name}
                      onSave={(name) =>
                        updateBaseUrlDefinition(project.id, def.id, { name })
                      }
                      className="flex-1 text-xs font-medium"
                      display={() => (
                        <span className="cursor-text rounded-sm px-1 py-0.5 hover:bg-accent/80">
                          {def.name}
                        </span>
                      )}
                    />
                    {isDefault && (
                      <Badge variant="success">默认</Badge>
                    )}
                    <Tooltip
                      content={isDefault ? '已是默认 baseUrl' : '设为默认 baseUrl'}
                    >
                      <Button
                        variant="ghost"
                        size="icon"
                        className={cn(
                          'h-6 w-6',
                          isDefault
                            ? 'text-warning-foreground'
                            : 'text-muted-foreground/60 hover:text-warning-foreground',
                        )}
                        onClick={() =>
                          setDefaultBaseUrlDefinition(
                            project.id,
                            isDefault ? null : def.id,
                          )
                        }
                        aria-label={isDefault ? '取消默认 baseUrl' : '设为默认 baseUrl'}
                      >
                        <Star className="h-3.5 w-3.5" fill={isDefault ? 'currentColor' : 'none'} />
                      </Button>
                    </Tooltip>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6 text-muted-foreground/70 hover:text-destructive"
                      onClick={() => {
                        if (
                          window.confirm(
                            `确认删除 baseUrl「${def.name}」？依赖它的接口会回落到项目默认。`,
                          )
                        ) {
                          deleteBaseUrlDefinition(project.id, def.id);
                        }
                      }}
                      aria-label={`删除 baseUrl ${def.name}`}
                      title="删除"
                    >
                      <Trash2 className="h-3 w-3" />
                    </Button>
                  </li>
                );
              })}
            </ul>
          )}
          <div className="mt-2 grid grid-cols-[1fr_auto] items-center gap-1.5">
            <Input
              value={newDefName}
              onChange={(e) => setNewDefName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && newDefName.trim()) {
                  addBaseUrlDefinition(project.id, newDefName.trim());
                  setNewDefName('');
                }
              }}
              placeholder="新 baseUrl 名称（如：鉴权服务）"
              className="h-7 text-[11px]"
            />
            <Button
              variant="outline"
              size="xs"
              className="h-7"
              disabled={!newDefName.trim()}
              onClick={() => {
                addBaseUrlDefinition(project.id, newDefName.trim());
                setNewDefName('');
              }}
            >
              <Plus className="mr-1 h-3 w-3" /> 新建
            </Button>
          </div>
        </Section>

        <Section
          title="模块"
          right={
            <span className="text-[10px] text-muted-foreground">{modules.length} 个</span>
          }
        >
          {modules.length === 0 ? (
            <EmptyHint text="该项目还没有模块。" />
          ) : (
            <ul className="space-y-0.5">
              {modules.map((m) => {
                const colsInMod = collections.filter((c) => c.moduleId === m.id);
                const epCount = endpoints.filter((e) =>
                  colsInMod.some((c) => c.id === e.collectionId),
                ).length;
                return (
                  <ModuleRow
                    key={m.id}
                    module={m}
                    collectionCount={colsInMod.length}
                    endpointCount={epCount}
                    collections={colsInMod}
                    allEndpoints={endpoints}
                    expanded={moduleExp.isExpanded(m.id)}
                    onRename={(name) => updateModule(m.id, { name })}
                    onToggleExpand={() => moduleExp.toggle(m.id)}
                    onRenameCollection={(cId, name) => updateCollection(cId, { name })}
                    onToggleCollection={(cId) => collectionExp.toggle(cId)}
                    isCollectionExpanded={(cId) => collectionExp.isExpanded(cId)}
                    onOpenEndpoint={(eId) => openTab('endpoint', eId)}
                    editState={editState}
                  />
                );
              })}
            </ul>
          )}
        </Section>

        <Section
          title="环境变量"
          right={
            <Button
              variant="ghost"
              size="xs"
              className="h-6 gap-1"
              onClick={() => addEnvironment(project.id, '新环境')}
            >
              <Plus className="h-3 w-3" /> 新建
            </Button>
          }
        >
          {environments.length === 0 ? (
            <EmptyHint text="还没有环境。点击右上角新建一个。" />
          ) : (
            <div className="space-y-3">
              {environments.map((env) => (
                <EnvironmentBlock
                  key={env.id}
                  envId={env.id}
                  name={env.name}
                  baseUrlDefs={project.baseUrlDefinitions}
                  baseUrls={env.baseUrls}
                  isActive={env.isActive}
                  variables={env.variables}
                  onRename={(name) => updateEnvironment(env.id, { name })}
                  onSetBaseUrl={(defId, url) =>
                    setEnvironmentBaseUrl(env.id, defId, url)
                  }
                  onActivate={() => setEnvironmentActive(env.id)}
                  onSetVar={(k, v) => setEnvironmentVar(env.id, k, v)}
                  onDelete={() => {
                    if (window.confirm(`确认删除环境「${env.name}」？依赖它的接口会回落到项目默认 baseUrl。`)) {
                      deleteEnvironment(env.id);
                    }
                  }}
                />
              ))}
              <Button
                variant="outline"
                size="sm"
                className="h-7 gap-1.5"
                onClick={() => setActiveProject(project.id)}
              >
                切换为当前项目
              </Button>
            </div>
          )}
        </Section>
      </div>
    </div>
  );
}

// ----------------------------------------------------------------
//  Module
// ----------------------------------------------------------------

function ModuleSettings({ moduleId, editState }: { moduleId: string; editState: EditCounter }) {
  const mod = useAppStore((s) => s.modules.find((m) => m.id === moduleId));
  const project = useAppStore((s) =>
    mod ? s.projects.find((p) => p.id === mod.projectId) : undefined,
  );
  // Raw references only — sorting belongs in the component.
  const allCollections = useAppStore((s) => s.collections);
  const endpoints = useAppStore((s) => s.endpoints);
  const collections = useMemo(
    () =>
      allCollections
        .filter((c) => c.moduleId === moduleId)
        .sort((a, b) => a.sortOrder - b.sortOrder),
    [allCollections, moduleId],
  );
  const updateModule = useAppStore((s) => s.updateModule);
  const openTab = useAppStore((s) => s.openTab);
  const updateCollection = useAppStore((s) => s.updateCollection);
  const openNewItem = useAppStore((s) => s.openNewItem);
  // Local-only expansion (see useLocalExpansion in EntitySettings).
  const collectionExp = useLocalExpansion();

  if (!mod) return <NotFound label="模块" />;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SettingsHeader
        icon={<FolderOpen className="h-4 w-4 text-warning" />}
        badge="模块"
        breadcrumb={project ? { label: project.name, onClick: () => openTab('project', project.id) } : undefined}
        title={mod.name}
        onTitleChange={(name) => updateModule(mod.id, { name })}
        description={mod.description}
        onDescriptionChange={(description) => updateModule(mod.id, { description })}
        onEditStart={editState.onStart}
        onEditEnd={editState.onEnd}
      />

      <div className="min-h-0 flex-1 overflow-y-auto">
        <Section
          title="集合"
          right={
            <Button
              variant="ghost"
              size="xs"
              className="h-6 gap-1"
              onClick={() =>
                openNewItem({ type: 'collection', parentModuleId: mod.id })
              }
            >
              <Plus className="h-3 w-3" /> 新建集合
            </Button>
          }
        >
          {collections.length === 0 ? (
            <EmptyHint text="该模块下还没有集合。" />
          ) : (
            <ul className="space-y-0.5">
              {collections.map((c) => {
                const colEps = endpoints
                  .filter((e) => e.collectionId === c.id)
                  .sort((a, b) => a.sortOrder - b.sortOrder);
                return (
                  <CollectionRow
                    key={c.id}
                    collection={c}
                    endpointCount={colEps.length}
                    endpoints={colEps}
                    expanded={collectionExp.isExpanded(c.id)}
                    onRename={(name) => updateCollection(c.id, { name })}
                    onToggleExpand={() => collectionExp.toggle(c.id)}
                    onOpenEndpoint={(eId) => openTab('endpoint', eId)}
                    editState={editState}
                  />
                );
              })}
            </ul>
          )}
        </Section>
      </div>
    </div>
  );
}

// ----------------------------------------------------------------
//  Collection
// ----------------------------------------------------------------

function CollectionSettings({ collectionId, editState }: { collectionId: string; editState: EditCounter }) {
  const col = useAppStore((s) => s.collections.find((c) => c.id === collectionId));
  const mod = useAppStore((s) =>
    col ? s.modules.find((m) => m.id === col.moduleId) : undefined,
  );
  const project = useAppStore((s) =>
    mod ? s.projects.find((p) => p.id === mod.projectId) : undefined,
  );
  // Raw endpoints reference; sort/filter in component to keep Zustand v5
  // happy (no array allocation in selectors).
  const allEndpoints = useAppStore((s) => s.endpoints);
  const endpoints = useMemo(
    () =>
      allEndpoints
        .filter((e) => e.collectionId === collectionId)
        .sort((a, b) => a.sortOrder - b.sortOrder),
    [allEndpoints, collectionId],
  );
  const updateCollection = useAppStore((s) => s.updateCollection);
  const openTab = useAppStore((s) => s.openTab);
  const openNewItem = useAppStore((s) => s.openNewItem);
  const updateEndpoint = useAppStore((s) => s.updateEndpoint);

  if (!col) return <NotFound label="集合" />;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SettingsHeader
        icon={<FolderOpen className="h-4 w-4 text-primary" />}
        badge="集合"
        breadcrumbs={[
          ...(project
            ? [{ label: project.name, onClick: () => openTab('project', project.id) }]
            : []),
          ...(mod
            ? [{ label: mod.name, onClick: () => openTab('module', mod.id) }]
            : []),
        ]}
        title={col.name}
        onTitleChange={(name) => updateCollection(col.id, { name })}
        description={col.description}
        onDescriptionChange={(description) => updateCollection(col.id, { description })}
        onEditStart={editState.onStart}
        onEditEnd={editState.onEnd}
      />

      <div className="min-h-0 flex-1 overflow-y-auto">
        <Section
          title="接口"
          right={
            <Button
              variant="ghost"
              size="xs"
              className="h-6 gap-1"
              onClick={() =>
                openNewItem({
                  type: 'endpoint',
                  parentModuleId: col.moduleId,
                  parentCollectionId: col.id,
                })
              }
            >
              <Plus className="h-3 w-3" /> 新建接口
            </Button>
          }
        >
          {endpoints.length === 0 ? (
            <EmptyHint text="该集合下还没有接口。" />
          ) : (
            <ul className="space-y-0.5">
              {endpoints.map((ep) => (
                <EndpointRow
                  key={ep.id}
                  endpoint={ep}
                  onOpen={() => openTab('endpoint', ep.id)}
                  onRename={(name) => updateEndpoint(ep.id, { name })}
                  editState={editState}
                />
              ))}
            </ul>
          )}
        </Section>
      </div>
    </div>
  );
}

// ----------------------------------------------------------------
//  Shared header / sections / fields
// ----------------------------------------------------------------

interface Crumb {
  label: string;
  onClick: () => void;
}

interface SettingsHeaderProps {
  icon: React.ReactNode;
  badge: string;
  title: string;
  onTitleChange: (name: string) => void;
  description?: string;
  onDescriptionChange: (description: string) => void;
  breadcrumb?: Crumb;
  breadcrumbs?: Crumb[];
  onEditStart?: () => void;
  onEditEnd?: () => void;
}

function SettingsHeader({
  icon,
  badge,
  title,
  onTitleChange,
  description,
  onDescriptionChange,
  breadcrumb,
  breadcrumbs,
  onEditStart,
  onEditEnd,
}: SettingsHeaderProps) {
  const crumbs = breadcrumbs ?? (breadcrumb ? [breadcrumb] : []);
  const [editingTitle, setEditingTitle] = useState(false);
  const [titleDraft, setTitleDraft] = useState(title);

  // Re-sync the local draft when the underlying entity changes (e.g. user
  // renames it from the tree via inline edit) so a stale draft never
  // overwrites a fresh value.
  useEffect(() => {
    if (!editingTitle) setTitleDraft(title);
  }, [title, editingTitle]);

  // Notify the dirty tracker (tab strip) when the title moves in/out of
  // edit mode. See EntitySettings' EditCounter for the consumer.
  useEffect(() => {
    if (editingTitle) onEditStart?.();
    else onEditEnd?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editingTitle]);

  return (
    <div className="flex shrink-0 flex-col gap-2 border-b border-border px-5 py-3">
      <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wider text-muted-foreground">
        {icon}
        <span>{badge}</span>
        {crumbs.map((c, i) => (
          <span key={i} className="inline-flex items-center gap-1.5">
            <span className="text-muted-foreground/40">/</span>
            <button
              type="button"
              onClick={c.onClick}
              className="max-w-[160px] truncate normal-case text-muted-foreground hover:text-foreground"
            >
              {c.label}
            </button>
          </span>
        ))}
      </div>
      <div className="flex items-center gap-2">
        {editingTitle ? (
          <Input
            value={titleDraft}
            onChange={(e) => setTitleDraft(e.target.value)}
            onBlur={() => {
              const next = titleDraft.trim();
              if (next && next !== title) onTitleChange(next);
              else setTitleDraft(title);
              setEditingTitle(false);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                (e.target as HTMLInputElement).blur();
              } else if (e.key === 'Escape') {
                setTitleDraft(title);
                setEditingTitle(false);
              }
            }}
            className="h-8 max-w-md text-sm font-semibold"
            autoFocus
          />
        ) : (
          <h1
            onDoubleClick={() => setEditingTitle(true)}
            className="cursor-text truncate rounded-sm px-1 py-0.5 text-sm font-semibold hover:bg-accent/60"
            title="双击重命名"
          >
            {title}
          </h1>
        )}
      </div>
      <DescriptionField
        value={description ?? ''}
        onChange={onDescriptionChange}
        placeholder={`一句话说明这个${badge}的用途…`}
      />
    </div>
  );
}

function DescriptionField({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
}) {
  const [focused, setFocused] = useState(false);
  return (
    <input
      value={value}
      onChange={(e) => onChange(e.target.value)}
      onFocus={() => setFocused(true)}
      onBlur={() => setFocused(false)}
      placeholder={placeholder}
      className={cn(
        'h-7 w-full max-w-2xl rounded-sm border border-transparent bg-transparent px-1 text-xs text-muted-foreground',
        'hover:border-border/60 focus:border-input focus:bg-background focus:text-foreground',
        'focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-1 focus:ring-offset-background',
      )}
    />
  );
}

// ----------------------------------------------------------------
//  Environment block — used inside project settings
// ----------------------------------------------------------------

function EnvironmentBlock({
  envId,
  name,
  baseUrlDefs,
  baseUrls,
  isActive,
  variables,
  onRename,
  onSetBaseUrl,
  onActivate,
  onSetVar,
  onDelete,
}: {
  envId: string;
  name: string;
  /**
   * The project's baseUrl definitions — one input row per def.
   * Render is the same regardless of which def is "default"; the
   * default only affects the picker's "项目默认" option, not this
   * env-scoped form.
   */
  baseUrlDefs: import('@/types/domain').BaseUrlDefinition[];
  /**
   * `env.baseUrls` map: defId → URL for this environment.
   */
  baseUrls: Record<string, string>;
  isActive: boolean;
  variables: Record<string, string>;
  onRename: (name: string) => void;
  onSetBaseUrl: (defId: string, url: string) => void;
  onActivate: () => void;
  onSetVar: (k: string, v: string) => void;
  onDelete: () => void;
}) {
  const [newKey, setNewKey] = useState('');
  const [newValue, setNewValue] = useState('');
  const [editingName, setEditingName] = useState(false);
  const [nameDraft, setNameDraft] = useState(name);

  // Keep the local rename draft in sync with the persisted name when
  // we're not actively editing (mirrors the SettingsHeader pattern).
  useEffect(() => {
    if (!editingName) setNameDraft(name);
  }, [name, editingName]);

  const entries = useMemo(() => Object.entries(variables), [variables]);

  return (
    <div
      className={cn(
        'rounded-md border bg-card/30 px-3 py-2',
        isActive ? 'border-primary/40' : 'border-border',
      )}
    >
      <div className="mb-2 flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <Hash className="h-3 w-3 shrink-0 text-muted-foreground" />
          {editingName ? (
            <Input
              value={nameDraft}
              onChange={(e) => setNameDraft(e.target.value)}
              onBlur={() => {
                const next = nameDraft.trim();
                if (next && next !== name) onRename(next);
                else setNameDraft(name);
                setEditingName(false);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
                else if (e.key === 'Escape') {
                  setNameDraft(name);
                  setEditingName(false);
                }
              }}
              className="h-6 w-32 font-mono text-xs"
              autoFocus
            />
          ) : (
            <span
              onDoubleClick={() => setEditingName(true)}
              className="cursor-text rounded-sm px-1 py-0.5 font-mono text-xs hover:bg-accent/60"
              title="双击重命名"
            >
              {name}
            </span>
          )}
          {isActive && (
            <Badge variant="success">active</Badge>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {!isActive && (
            <Button variant="outline" size="xs" className="h-6" onClick={onActivate}>
              设为活动
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6 text-muted-foreground/70 hover:text-destructive"
            onClick={onDelete}
            aria-label={`删除环境 ${name}`}
            title="删除环境"
          >
            <Trash2 className="h-3 w-3" />
          </Button>
        </div>
      </div>
      {baseUrlDefs.length === 0 ? (
        <div className="mb-2 rounded-sm border border-dashed border-border/60 px-2 py-1.5 text-[11px] text-muted-foreground/80">
          该项目还没有 baseUrl 定义 —— 回到上方「BaseUrl 定义」一节新增。
        </div>
      ) : (
        <ul className="mb-2 space-y-1.5">
          {baseUrlDefs.map((def) => (
            <li key={def.id} className="flex items-center gap-1.5">
              <Globe className="h-3 w-3 shrink-0 text-muted-foreground" />
              <span
                className="w-24 shrink-0 truncate text-[10px] text-muted-foreground/80"
                title={def.name}
              >
                {def.name}
              </span>
              <Input
                value={baseUrls[def.id] ?? ''}
                onChange={(e) => onSetBaseUrl(def.id, e.target.value)}
                placeholder="https://api.example.com"
                className="h-7 flex-1 font-mono text-[11px]"
                data-testid={`env-baseurl-${envId}-${def.id}`}
              />
            </li>
          ))}
        </ul>
      )}
      {entries.length > 0 && (
        <ul className="mb-2 space-y-1">
          {entries.map(([k, v]) => (
            <li
              key={k}
              className="grid grid-cols-[140px_1fr] items-center gap-2 rounded-sm bg-background/50 px-2 py-1 text-[11px]"
            >
              <span className="truncate font-mono text-muted-foreground">{k}</span>
              <span className="truncate font-mono">{v}</span>
            </li>
          ))}
        </ul>
      )}
      <div className="grid grid-cols-[140px_1fr_auto] items-center gap-1.5">
        <Input
          value={newKey}
          onChange={(e) => setNewKey(e.target.value)}
          placeholder="变量名"
          className="h-7 text-[11px]"
        />
        <Input
          value={newValue}
          onChange={(e) => setNewValue(e.target.value)}
          placeholder="值"
          className="h-7 text-[11px]"
        />
        <Button
          variant="outline"
          size="xs"
          className="h-7"
          disabled={!newKey.trim()}
          onClick={() => {
            onSetVar(newKey.trim(), newValue);
            setNewKey('');
            setNewValue('');
          }}
        >
          添加
        </Button>
      </div>
    </div>
  );
}

// ----------------------------------------------------------------
//  Sub-list row components — used by the three settings panels to
//  render their children inline. Per the new UX rule, the rows are
//  NOT clickable to navigate. Instead:
//    - Click on the row body (anywhere outside the name input)
//      toggles expand/collapse.
//    - Double-click on the name enters inline rename.
//  The only exception is the endpoint row, which keeps the
//  double-click → "open in test panel" behaviour because endpoints
//  have no inline sub-content of their own — opening the test view
//  is the natural way to drill in.
// ----------------------------------------------------------------

function ModuleRow({
  module,
  collectionCount,
  endpointCount,
  collections,
  allEndpoints,
  expanded,
  onRename,
  onToggleExpand,
  onRenameCollection,
  onToggleCollection,
  isCollectionExpanded,
  onOpenEndpoint,
  editState,
}: {
  module: import('@/types/domain').Module;
  collectionCount: number;
  endpointCount: number;
  collections: import('@/types/domain').Collection[];
  allEndpoints: import('@/types/domain').Endpoint[];
  /**
   * Whether this module row is expanded in the *settings* pane.
   * Independent from `module.expanded` (which the tree uses) so the
   * two surfaces don't drag each other around.
   */
  expanded: boolean;
  onRename: (name: string) => void;
  onToggleExpand: () => void;
  onRenameCollection: (cId: string, name: string) => void;
  onToggleCollection: (cId: string) => void;
  /**
   * Lookup for whether a child collection is expanded in the
   * settings pane. Same independence rationale as `expanded` above.
   */
  isCollectionExpanded: (cId: string) => boolean;
  onOpenEndpoint: (eId: string) => void;
  editState: EditCounter;
}) {
  return (
    <li className="rounded-sm">
      <div
        role="button"
        tabIndex={0}
        onClick={onToggleExpand}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            onToggleExpand();
          }
        }}
        className="flex cursor-pointer select-none items-center gap-1.5 rounded-sm px-1 py-1 text-xs hover:bg-accent/60"
      >
        <span className="flex h-4 w-4 shrink-0 items-center justify-center text-muted-foreground/60">
          {expanded ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
        </span>
        {expanded ? (
          <FolderOpen className="h-3.5 w-3.5 shrink-0 text-warning/80" />
        ) : (
          <FolderClosed className="h-3.5 w-3.5 shrink-0 text-warning/80" />
        )}
        <InlineEdit
          value={module.name}
          onSave={onRename}
          className="flex-1 font-medium"
          display={() => (
            <span className="cursor-text rounded-sm px-1 py-0.5 hover:bg-accent/80">
              {module.name}
            </span>
          )}
        />
        <span className="shrink-0 text-[10px] text-muted-foreground">
          {collectionCount} / {endpointCount}
        </span>
      </div>
      {expanded && (
        <ul className="ml-4 mt-0.5 space-y-0.5 border-l border-border/40 pl-2">
          {collections.length === 0 ? (
            <li className="px-1 py-1 text-[10px] italic text-muted-foreground/60">
              （暂无集合）
            </li>
          ) : (
            collections.map((c) => {
              const colEps = allEndpoints
                .filter((e) => e.collectionId === c.id)
                .sort((a, b) => a.sortOrder - b.sortOrder);
              return (
                <CollectionRow
                  key={c.id}
                  collection={c}
                  endpointCount={colEps.length}
                  endpoints={colEps}
                  expanded={isCollectionExpanded(c.id)}
                  onRename={(name) => onRenameCollection(c.id, name)}
                  onToggleExpand={() => onToggleCollection(c.id)}
                  onOpenEndpoint={onOpenEndpoint}
                  editState={editState}
                />
              );
            })
          )}
        </ul>
      )}
    </li>
  );
}

function CollectionRow({
  collection,
  endpointCount,
  endpoints,
  expanded,
  onRename,
  onToggleExpand,
  onOpenEndpoint,
  editState,
}: {
  collection: import('@/types/domain').Collection;
  endpointCount: number;
  endpoints: import('@/types/domain').Endpoint[];
  expanded: boolean;
  onRename: (name: string) => void;
  onToggleExpand: () => void;
  onOpenEndpoint: (eId: string) => void;
  editState: EditCounter;
}) {
  return (
    <li className="rounded-sm">
      <div
        role="button"
        tabIndex={0}
        onClick={onToggleExpand}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            onToggleExpand();
          }
        }}
        className="flex cursor-pointer select-none items-center gap-1.5 rounded-sm px-1 py-1 text-xs hover:bg-accent/60"
      >
        <span className="flex h-4 w-4 shrink-0 items-center justify-center text-muted-foreground/60">
          {expanded ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
        </span>
        {expanded ? (
          <FolderOpen className="h-3.5 w-3.5 shrink-0 text-primary/80" />
        ) : (
          <FolderClosed className="h-3.5 w-3.5 shrink-0 text-primary/80" />
        )}
        <InlineEdit
          value={collection.name}
          onSave={onRename}
          className="flex-1 font-medium"
          display={() => (
            <span className="cursor-text rounded-sm px-1 py-0.5 hover:bg-accent/80">
              {collection.name}
            </span>
          )}
        />
        <span className="shrink-0 text-[10px] text-muted-foreground">
          {endpointCount} 个接口
        </span>
      </div>
      {expanded && (
        <ul className="ml-4 mt-0.5 space-y-0.5 border-l border-border/40 pl-2">
          {endpoints.length === 0 ? (
            <li className="px-1 py-1 text-[10px] italic text-muted-foreground/60">
              （暂无接口）
            </li>
          ) : (
            endpoints.map((ep) => (
              <EndpointRow
                key={ep.id}
                endpoint={ep}
                onOpen={() => onOpenEndpoint(ep.id)}
                onRename={() => {
                  /* inline rename of an endpoint is done from the
                     endpoint test view's title bar — keep this row
                     display-only here */
                }}
                readOnlyName
                editState={editState}
              />
            ))
          )}
        </ul>
      )}
    </li>
  );
}

function EndpointRow({
  endpoint,
  onOpen,
  onRename,
  readOnlyName,
  editState,
}: {
  endpoint: import('@/types/domain').Endpoint;
  onOpen: () => void;
  onRename: (name: string) => void;
  readOnlyName?: boolean;
  editState: EditCounter;
}) {
  return (
    <li
      className="flex cursor-pointer select-none items-center gap-1.5 rounded-sm px-1 py-1 text-xs hover:bg-accent/60"
      onDoubleClick={onOpen}
      onClick={(e) => {
        // Single click does nothing — keep the row display-only.
        // (Click is reserved for the test view's activate handler.)
        e.preventDefault();
      }}
      title="双击打开调试界面"
    >
      <span
        className="method-badge shrink-0"
        style={{ backgroundColor: methodColorVar(endpoint.method) }}
      >
        {endpoint.method}
      </span>
      {readOnlyName ? (
        <span className="flex-1 truncate text-muted-foreground">{endpoint.name}</span>
      ) : (
        <InlineEdit
          value={endpoint.name}
          onSave={onRename}
          className="flex-1 truncate font-medium"
          display={() => (
            <span className="cursor-text truncate rounded-sm px-1 py-0.5 hover:bg-accent/80">
              {endpoint.name}
            </span>
          )}
        />
      )}
      <span
        className="shrink-0 truncate font-mono text-[10px] text-muted-foreground/70"
        title={endpoint.url}
      >
        {endpoint.url}
      </span>
    </li>
  );
}
