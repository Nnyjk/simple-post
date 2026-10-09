/**
 * EntitySettings — the pane for project / module / collection tabs.
 *
 * The full configuration UIs (baseUrl definitions, environment variables,
 * child collection lists, etc.) used to live here. They overlapped with
 * the central Settings drawer (env / connection / appearance / request)
 * so we collapsed them into one place and now the right pane only shows
 * the entity name + a hint pointing to the new home.
 *
 * The project card is the one exception: it carries the project's own
 * rename + delete, because the project used to be a row in the tree and
 * that row was the only place those actions were reachable. With the
 * project out of the tree (it is selected in the top bar / welcome
 * picker, not rendered as a node), this card is where the user lands
 * after entering a project — so it is the natural home for them.
 *
 * Endpoint tabs route to `HttpTester` instead of this component, so the
 * switch's default branch returns null.
 */

import { useMemo, useState } from 'react';
import { Package, FolderOpen, FolderClosed, Pencil, Trash2 } from 'lucide-react';
import { useAppStore, type TabKind } from '@/stores/app-store';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { InlineEdit } from '@/components/ui/InlineEdit';
import { cn, focusRing } from '@/lib/utils';

interface EntitySettingsProps {
  kind: TabKind;
  entityId: string;
}

export function EntitySettings({ kind, entityId }: EntitySettingsProps) {
  switch (kind) {
    case 'project':
      return <ProjectSettings projectId={entityId} />;
    case 'module':
      return <ModuleSettings moduleId={entityId} />;
    case 'collection':
      return <CollectionSettings collectionId={entityId} />;
    default:
      // Endpoint tabs route to HttpTester instead.
      return null;
  }
}

// ----------------------------------------------------------------------------
//  Sub-components (display only)
// ----------------------------------------------------------------------------

function ProjectSettings({ projectId }: { projectId: string }) {
  const project = useAppStore((s) => s.projects.find((p) => p.id === projectId));
  const projects = useAppStore((s) => s.projects);
  const nodes = useAppStore((s) => s.nodes);
  const updateProject = useAppStore((s) => s.updateProject);
  const deleteProject = useAppStore((s) => s.deleteProject);
  const [confirmDelete, setConfirmDelete] = useState(false);

  // One pass over the flat node list, bucketed by the denormalised
  // `rootId` — a `useMemo` rather than a selector, because a selector
  // that allocates would re-render on every store write (Zustand v5).
  const tally = useMemo(() => {
    let modules = 0;
    let collections = 0;
    let endpoints = 0;
    for (const n of nodes) {
      if (n.rootId !== projectId) continue;
      if (n.kind === 'module') modules += 1;
      else if (n.kind === 'collection') collections += 1;
      else endpoints += 1;
    }
    return { modules, collections, endpoints };
  }, [nodes, projectId]);

  if (!project) return <NotFound label="项目" />;

  // Deleting the last project would leave an app with nothing to enter,
  // so the tree used to disable it — the guard moves here with the
  // action. `deleteProject` also re-points `activeProjectId` at a
  // surviving project, so the tree does not go blank behind the dialog.
  const isOnlyProject = projects.length <= 1;

  return (
    <>
      <div className="flex h-full min-h-0 items-center justify-center overflow-y-auto bg-card/30 p-8">
        <div className="w-full max-w-sm text-center">
          <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-muted/40">
            <Package className="h-5 w-5 text-hue-project" />
          </div>
          <div className="label-caps">项目</div>
          <div className="mt-1 flex items-center justify-center gap-1">
            <InlineEdit
              value={project.name}
              onSave={(name) => void updateProject(project.id, { name })}
              ariaLabel="项目名称"
              className="min-w-0 max-w-full"
              display={({ open }) => (
                <button
                  type="button"
                  onClick={open}
                  title="点击重命名"
                  className={cn(
                    'group inline-flex min-w-0 max-w-full items-center gap-1 rounded px-1 py-0.5 hover:bg-accent/60',
                    focusRing,
                  )}
                >
                  <h1 className="truncate text-lg font-semibold text-foreground">{project.name}</h1>
                  <Pencil className="h-3 w-3 shrink-0 opacity-0 transition-opacity group-hover:opacity-60" />
                </button>
              )}
            />
          </div>
          {project.description && (
            <p className="mt-1 text-xs text-muted-foreground">{project.description}</p>
          )}
          <p className="mt-3 font-mono text-[11px] tabular-nums text-muted-foreground">
            {tally.modules} 模块 · {tally.collections} 集合 · {tally.endpoints} 接口
          </p>
          <p className="mt-3 text-xs text-muted-foreground">
            环境与连接配置已统一到 <kbd className="kbd">设置</kbd> → 环境
          </p>

          <div className="mt-6 flex items-center justify-center gap-2 border-t border-border pt-4">
            <Tooltip
              content={isOnlyProject ? '至少保留一个项目' : '删除该项目及其全部内容'}
              side="top"
            >
              <span>
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={isOnlyProject}
                  onClick={() => setConfirmDelete(true)}
                  className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  删除项目
                </Button>
              </span>
            </Tooltip>
          </div>
        </div>
      </div>

      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title={`删除项目「${project.name}」？`}
        description={`此操作不可撤销，将同时删除 ${tally.modules} 个模块、${tally.collections} 个集合、${tally.endpoints} 个接口及其环境配置。`}
        destructive
        confirmText="删除"
        onConfirm={() => void deleteProject(project.id)}
      />
    </>
  );
}

function ModuleSettings({ moduleId }: { moduleId: string }) {
  const mod = useAppStore((s) => s.nodes.find((n) => n.id === moduleId));
  if (!mod) return <NotFound label="模块" />;
  return (
    <EntityCard
      icon={<FolderOpen className="h-5 w-5 text-hue-module" />}
      badge="模块"
      name={mod.name}
    />
  );
}

function CollectionSettings({ collectionId }: { collectionId: string }) {
  const col = useAppStore((s) => s.nodes.find((n) => n.id === collectionId));
  if (!col) return <NotFound label="集合" />;
  return (
    <EntityCard
      icon={<FolderClosed className="h-5 w-5 text-hue-collection" />}
      badge="集合"
      name={col.name}
    />
  );
}

// ----------------------------------------------------------------------------
//  Shared display
// ----------------------------------------------------------------------------

function EntityCard({ icon, badge, name }: { icon: React.ReactNode; badge: string; name: string }) {
  return (
    <div className="flex h-full min-h-0 items-center justify-center bg-card/30 p-8">
      <div className="w-full max-w-sm text-center">
        <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-muted/40">
          {icon}
        </div>
        <div className="label-caps">{badge}</div>
        <h1 className="mt-1 truncate text-lg font-semibold text-foreground" title={name}>
          {name}
        </h1>
        <p className="mt-3 text-xs text-muted-foreground">
          配置已统一到 <kbd className="kbd">设置</kbd> → 环境
        </p>
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
