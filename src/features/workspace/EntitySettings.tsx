/**
 * EntitySettings — the pane behind a project / module / collection tab.
 *
 * ONE COLUMN, NO TABS
 * -------------------
 * An entity tab used to be a card: an icon, a name, and a sentence pointing
 * at 设置 → 环境. Then it became a two-layer pane with a switcher in the
 * header. Both were wrong for the same reason — they made the user *choose*
 * between "what this thing is" and "how this thing is configured", when
 * both are short enough to just be there. Two fields, then an env list you
 * scroll through. So the pane is a single waterfall now: identity in the
 * header, then 基础信息, then whatever configuration belongs to that kind,
 * stacked top to bottom.
 *
 * That is not only a layout preference. The layers hid things: with
 * 基础信息 selected, a module's addresses were invisible and the only clue
 * they existed was a summary line ("1 个环境已填写") that the pane then had
 * to explain. One column means the module's env switcher and its baseUrl
 * table are simply below its name, where you scroll into them.
 *
 * WHAT IS NOT RE-IMPLEMENTED HERE
 * -------------------------------
 * The 环境 block is the *same* `EnvironmentSection` the settings tab mounts,
 * behind the same store-connected `ProjectEnvSection`; the 服务 block is
 * `ModuleBaseUrlTable` in `module` mode. Two hosts, one implementation —
 * see those files for why the rules live there and not here.
 *
 * Endpoint tabs route to `HttpTester` instead of this component, so the
 * switch's default branch returns null.
 */

import { lazy, Suspense, useState, type ReactNode } from 'react';
import { FolderClosed, FolderOpen, Package, Trash2 } from 'lucide-react';
import { useAppStore, type TabKind } from '@/stores/app-store';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { IconTile } from '@/components/ui/icon-tile';
import { AutoSaveInput, AutoSaveTextarea, InfoField } from './EntityInfoFields';

// Both configuration blocks are separate chunks. `EntitySettings` is itself
// lazy (mounted on any entity tab), but the env section drags in its
// switcher, both tables and the dialog plumbing. They are mounted with the
// pane rather than behind a click, so `lazy()` here only splits the chunk —
// it does not defer anything the user cannot see scrolling.
const ProjectEnvSection = lazy(() =>
  import('@/features/settings/sections/ProjectEnvSection').then((m) => ({
    default: m.ProjectEnvSection,
  })),
);
const ModuleServiceSection = lazy(() =>
  import('@/features/settings/sections/ModuleServiceSection').then((m) => ({
    default: m.ModuleServiceSection,
  })),
);

interface EntitySettingsProps {
  kind: TabKind;
  entityId: string;
}

export function EntitySettings({ kind, entityId }: EntitySettingsProps) {
  switch (kind) {
    case 'project':
      return <ProjectWorkspace projectId={entityId} />;
    case 'module':
      return <ModuleWorkspace moduleId={entityId} />;
    case 'collection':
      return <CollectionWorkspace collectionId={entityId} />;
    default:
      // Endpoint tabs route to HttpTester instead.
      return null;
  }
}

// ----------------------------------------------------------------------------
//  Shared chrome
// ----------------------------------------------------------------------------

/**
 * The three kinds that own a tab pane. `HueConcept` is wider than that —
 * it also names colours for things that are not nodes — so the label map is
 * keyed by the narrower type rather than pretending to cover it all.
 */
type EntityKind = 'project' | 'module' | 'collection';

const KIND_LABEL: Record<EntityKind, string> = {
  project: '项目',
  module: '模块',
  collection: '集合',
};

/**
 * Identity header. Kept because it is the one thing that is true of the
 * whole pane: which entity everything below belongs to. On a long scroll it
 * is also the only fixed cue, since the tab strip is above the fold's
 * competition for attention.
 */
function EntityHeader({
  concept,
  icon,
  name,
}: {
  concept: EntityKind;
  icon: ReactNode;
  name: string;
}) {
  return (
    <header className="flex h-11 shrink-0 items-center gap-3 border-b border-border bg-card/40 px-6">
      <IconTile icon={icon} concept={concept} size="sm" />
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-semibold text-foreground" title={name}>
          {name}
        </div>
        <div className="label-caps">{KIND_LABEL[concept]}</div>
      </div>
    </header>
  );
}

/**
 * One titled block in the waterfall.
 *
 * The inset matches what `SettingsCategory` produces for the env block, so
 * a project tab reads as one column rather than as "our blocks" plus "their
 * block, indented differently".
 */
function EntitySection({
  title,
  hint,
  children,
}: {
  title: string;
  /** One line under the title. States a rule; does not restate the title. */
  hint?: string;
  children: ReactNode;
}) {
  return (
    <section className="px-6 pb-1 pt-5">
      <div className="mb-3">
        <h2 className="label-caps-strong">{title}</h2>
        {hint && <p className="mt-0.5 text-[11px] text-muted-foreground">{hint}</p>}
      </div>
      {children}
    </section>
  );
}

/** Suspense stand-in sized to the block it replaces. */
function BlockFallback() {
  return (
    <div className="flex min-h-32 items-center justify-center py-10 text-xs text-muted-foreground">
      加载中…
    </div>
  );
}

// ----------------------------------------------------------------------------
//  项目 — 基础信息 + 环境
// ----------------------------------------------------------------------------

function ProjectWorkspace({ projectId }: { projectId: string }) {
  const project = useAppStore((s) => s.projects.find((p) => p.id === projectId));
  if (!project) return <NotFound label="项目" />;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <EntityHeader concept="project" icon={<Package className="h-4 w-4" />} name={project.name} />
      <div className="min-h-0 flex-1 overflow-y-auto pb-6">
        <ProjectInfo projectId={projectId} />
        <EntitySection
          title="环境"
          hint="这个项目的环境变量与前置 URL · 与设置里的环境分区是同一份"
        >
          <Suspense fallback={<BlockFallback />}>
            <ProjectEnvSection />
          </Suspense>
        </EntitySection>
        <ProjectDangerZone projectId={projectId} />
      </div>
    </div>
  );
}

function ProjectInfo({ projectId }: { projectId: string }) {
  const project = useAppStore((s) => s.projects.find((p) => p.id === projectId));
  const updateProject = useAppStore((s) => s.updateProject);
  if (!project) return null;

  return (
    <EntitySection title="基础信息">
      <div className="rounded-lg border border-border/70 bg-card/40">
        <InfoField label="名称" hint="显示在资源树顶部、项目标签与欢迎页">
          <AutoSaveInput
            value={project.name}
            onSave={(name) => void updateProject(project.id, { name })}
            ariaLabel="项目名称"
            placeholder="项目名称"
          />
        </InfoField>
        <InfoField
          label="描述"
          hint="记下这个项目是什么 · Ctrl/⌘+Enter 保存，Esc 放弃"
          className="border-t border-border/60"
        >
          <AutoSaveTextarea
            value={project.description}
            onSave={(description) => void updateProject(project.id, { description })}
            ariaLabel="项目描述"
            placeholder="添加描述…"
          />
        </InfoField>
      </div>
    </EntitySection>
  );
}

function ProjectDangerZone({ projectId }: { projectId: string }) {
  const project = useAppStore((s) => s.projects.find((p) => p.id === projectId));
  const deleteProject = useAppStore((s) => s.deleteProject);
  const [confirmDelete, setConfirmDelete] = useState(false);

  if (!project) return null;

  return (
    <div className="px-6 pt-6">
      <div className="flex items-center justify-center border-t border-border pt-4">
        <Tooltip content="删除该项目及其全部内容" side="top">
          <span>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setConfirmDelete(true)}
              className="text-destructive hover:bg-destructive/10 hover:text-destructive"
            >
              <Trash2 className="h-3.5 w-3.5" />
              删除项目
            </Button>
          </span>
        </Tooltip>
      </div>

      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title={`删除项目「${project.name}」？`}
        description="此操作不可撤销，将同时删除该项目下的全部模块、集合、接口及其环境配置。"
        destructive
        confirmText="删除"
        onConfirm={() => void deleteProject(project.id)}
      />
    </div>
  );
}

// ----------------------------------------------------------------------------
//  模块 — 基础信息 + 服务
// ----------------------------------------------------------------------------

function ModuleWorkspace({ moduleId }: { moduleId: string }) {
  const mod = useAppStore((s) => s.nodes.find((n) => n.id === moduleId));
  const updateNode = useAppStore((s) => s.updateNode);
  if (!mod) return <NotFound label="模块" />;

  const Folder = mod.expanded ? FolderOpen : FolderClosed;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <EntityHeader concept="module" icon={<Folder className="h-4 w-4" />} name={mod.name} />
      <div className="min-h-0 flex-1 overflow-y-auto pb-6">
        <EntitySection title="基础信息">
          <div className="rounded-lg border border-border/70 bg-card/40">
            <InfoField label="名称" hint="显示在资源树与模块标签">
              <AutoSaveInput
                value={mod.name}
                onSave={(name) => void updateNode(mod.id, { name })}
                ariaLabel="模块名称"
                placeholder="模块名称"
              />
            </InfoField>
            <InfoField
              label="描述"
              hint="记下这个模块负责什么 · Ctrl/⌘+Enter 保存，Esc 放弃"
              className="border-t border-border/60"
            >
              <AutoSaveTextarea
                value={mod.description}
                onSave={(description) => void updateNode(mod.id, { description })}
                ariaLabel="模块描述"
                placeholder="添加描述…"
              />
            </InfoField>
          </div>
        </EntitySection>

        <EntitySection
          title="服务"
          hint="服务清单属于项目，这里填的是本模块自己的地址 · 各环境分别保存"
        >
          <Suspense fallback={<BlockFallback />}>
            <ModuleServiceSection moduleId={moduleId} />
          </Suspense>
        </EntitySection>
      </div>
    </div>
  );
}

// ----------------------------------------------------------------------------
//  集合 — 基础信息
// ----------------------------------------------------------------------------

function CollectionWorkspace({ collectionId }: { collectionId: string }) {
  const col = useAppStore((s) => s.nodes.find((n) => n.id === collectionId));
  const updateNode = useAppStore((s) => s.updateNode);
  if (!col) return <NotFound label="集合" />;

  const Folder = col.expanded ? FolderOpen : FolderClosed;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <EntityHeader concept="collection" icon={<Folder className="h-4 w-4" />} name={col.name} />
      <div className="min-h-0 flex-1 overflow-y-auto pb-6">
        <EntitySection title="基础信息">
          <div className="rounded-lg border border-border/70 bg-card/40">
            <InfoField label="名称" hint="显示在资源树与集合标签">
              <AutoSaveInput
                value={col.name}
                onSave={(name) => void updateNode(col.id, { name })}
                ariaLabel="集合名称"
                placeholder="集合名称"
              />
            </InfoField>
            <InfoField
              label="描述"
              hint="记下这组接口的用途 · Ctrl/⌘+Enter 保存，Esc 放弃"
              className="border-t border-border/60"
            >
              <AutoSaveTextarea
                value={col.description}
                onSave={(description) => void updateNode(col.id, { description })}
                ariaLabel="集合描述"
                placeholder="添加描述…"
              />
            </InfoField>
          </div>
        </EntitySection>
      </div>
    </div>
  );
}

function NotFound({ label }: { label: string }) {
  return (
    <div className="flex h-full min-h-0 items-center justify-center bg-card/30 p-8 text-sm text-muted-foreground">
      该{label}已不存在
    </div>
  );
}
