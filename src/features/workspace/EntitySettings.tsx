/**
 * EntitySettings — minimal display-only view for project / module / collection
 * tabs.
 *
 * The full configuration UIs (baseUrl definitions, environment variables,
 * child collection lists, etc.) used to live here. They overlapped with
 * the central Settings drawer (env / connection / appearance / request)
 * so we collapsed them into one place and now the right pane only shows
 * the entity name + a hint pointing to the new home.
 *
 * Endpoint tabs route to `HttpTester` instead of this component, so the
 * switch's default branch returns null.
 */

import { Package, FolderOpen, FolderClosed } from 'lucide-react';
import { useAppStore, type TabKind } from '@/stores/app-store';

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
  if (!project) return <NotFound label="项目" />;
  return (
    <EntityCard
      icon={<Package className="h-5 w-5 text-violet-400" />}
      badge="项目"
      name={project.name}
    />
  );
}

function ModuleSettings({ moduleId }: { moduleId: string }) {
  const mod = useAppStore((s) => s.modules.find((m) => m.id === moduleId));
  if (!mod) return <NotFound label="模块" />;
  return (
    <EntityCard
      icon={<FolderOpen className="h-5 w-5 text-warning" />}
      badge="模块"
      name={mod.name}
    />
  );
}

function CollectionSettings({ collectionId }: { collectionId: string }) {
  const col = useAppStore(
    (s) => s.collections.find((c) => c.id === collectionId),
  );
  if (!col) return <NotFound label="集合" />;
  return (
    <EntityCard
      icon={<FolderClosed className="h-5 w-5 text-primary" />}
      badge="集合"
      name={col.name}
    />
  );
}

// ----------------------------------------------------------------------------
//  Shared display
// ----------------------------------------------------------------------------

function EntityCard({
  icon,
  badge,
  name,
}: {
  icon: React.ReactNode;
  badge: string;
  name: string;
}) {
  return (
    <div className="flex h-full min-h-0 items-center justify-center bg-card/30 p-8">
      <div className="w-full max-w-sm text-center">
        <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-muted/40">
          {icon}
        </div>
        <div className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
          {badge}
        </div>
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