import type { MouseEvent as ReactMouseEvent } from 'react';
import { Package, Plus } from 'lucide-react';
import type { Project } from '@/types/domain';
import { cn } from '@/lib/utils';
import { Tooltip } from '@/components/ui/tooltip';
import { InlineEdit } from '@/components/ui/InlineEdit';
import { IconTile } from '@/components/ui/icon-tile';

export function ProjectRow({
  project,
  moduleCount,
  collectionCount,
  endpointCount,
  isEditing,
  onSelect,
  onStartEdit,
  onSaveEdit,
  onContextMenu,
  onPlusClick,
}: {
  project: Project;
  moduleCount: number;
  collectionCount: number;
  endpointCount: number;
  isEditing: boolean;
  onSelect: () => void;
  onStartEdit: () => void;
  onSaveEdit: (name: string) => void;
  onCancelEdit: () => void;
  onContextMenu: (e: ReactMouseEvent) => void;
  onPlusClick: (e: ReactMouseEvent) => void;
}) {
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onSelect();
        }
      }}
      onDoubleClick={(e) => {
        if (!isEditing) {
          e.preventDefault();
          e.stopPropagation();
          onStartEdit();
        }
      }}
      onContextMenu={onContextMenu}
      className={cn(
        'group mb-0.5 flex w-full cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-left text-xs transition-colors',
        'hover:bg-accent/60',
      )}
    >
      <IconTile concept="project" size="sm" icon={<Package className="h-3 w-3" />} />
      <InlineEdit
        value={project.name}
        onSave={onSaveEdit}
        className="flex-1"
        display={() => (
          <span className="flex-1 truncate font-medium text-foreground">{project.name}</span>
        )}
      />
      <span className="shrink-0 rounded bg-muted/60 px-1.5 font-mono text-[10px] tabular-nums text-muted-foreground">
        {moduleCount}/{collectionCount}/{endpointCount}
      </span>
      <div className="invisible flex shrink-0 items-center pr-1 group-hover:visible">
        <Tooltip content="新建" side="top">
          <button
            type="button"
            onClick={onPlusClick}
            onDoubleClick={(e) => e.stopPropagation()}
            aria-label="新建"
            className="flex h-5 w-5 items-center justify-center rounded-sm text-muted-foreground hover:bg-accent hover:text-foreground"
          >
            <Plus className="h-3 w-3" />
          </button>
        </Tooltip>
      </div>
    </div>
  );
}
