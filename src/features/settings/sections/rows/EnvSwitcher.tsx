import { Plus, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { IconTile } from '@/components/ui/icon-tile';
import type { Environment } from '@/types/domain';
import { envIcon } from '../env-utils';

/**
 * EnvSwitcher — top-of-section env picker.
 *
 * Renders unconditionally, including for a project with zero envs: the
 * trailing `新建环境` button is the section's only add affordance, so
 * gating the whole strip on a non-empty list is what used to leave a
 * cold-start project with no way to create its first environment.
 */
export function EnvSwitcher({
  environments,
  activeId,
  onPick,
  onRequestDelete,
  onAdd,
  addDisabled,
}: {
  environments: Environment[];
  activeId: string | null;
  onPick: (id: string) => void;
  onRequestDelete: (env: Environment) => void;
  onAdd: () => void;
  /** Create is in flight — block a second submit until it resolves. */
  addDisabled?: boolean;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      {environments.map((env) => {
        const isActive = env.id === activeId;
        return (
          <button
            key={env.id}
            type="button"
            role="tab"
            aria-selected={isActive}
            onClick={() => onPick(env.id)}
            className={cn(
              'group relative flex items-center gap-2 rounded-lg border px-2.5 py-1.5 text-xs transition-colors',
              // Coloured icon container matches the project's tier
              // language (sky = base / data, amber = config / builds,
              // violet = production-ish). Picked automatically from the
              // env name so users don't have to tag each one.
              isActive
                ? 'border-primary/40 bg-primary/10 text-foreground shadow-[inset_0_0_0_1px_hsl(var(--primary)/0.25)]'
                : 'border-border bg-card/30 text-muted-foreground hover:bg-accent/40 hover:text-foreground',
            )}
          >
            <IconTile
              size="xs"
              neutralBorder
              icon={envIcon(env.name)}
              className={
                isActive
                  ? 'border-primary/40 bg-primary/15 text-primary'
                  : 'border-border bg-muted/60 text-muted-foreground'
              }
            />
            <span className="font-mono font-medium">{env.name}</span>
            {environments.length > 1 && (
              <Tooltip content="删除环境" side="bottom">
                <span
                  role="button"
                  aria-label={`删除环境 ${env.name}`}
                  tabIndex={-1}
                  onClick={(e) => {
                    e.stopPropagation();
                    onRequestDelete(env);
                  }}
                  className={cn(
                    'ml-0.5 inline-flex h-4 w-4 cursor-pointer items-center justify-center rounded text-muted-foreground/60 opacity-0 hover:bg-destructive/20 hover:text-destructive',
                    'group-hover:opacity-100',
                    isActive && 'opacity-100',
                  )}
                >
                  <Trash2 className="h-3 w-3" />
                </span>
              </Tooltip>
            )}
          </button>
        );
      })}
      <Button
        variant="outline"
        size="sm"
        className="h-8 gap-1 border-dashed text-xs text-muted-foreground hover:text-foreground"
        onClick={onAdd}
        disabled={addDisabled}
      >
        <Plus className="h-3.5 w-3.5" />
        新建环境
      </Button>
    </div>
  );
}
