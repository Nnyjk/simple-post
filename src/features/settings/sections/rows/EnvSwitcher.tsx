import { Plus, Trash2 } from 'lucide-react';
import { type KeyboardEvent } from 'react';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { IconTile } from '@/components/ui/icon-tile';
import type { Environment } from '@/types/domain';
import { envIcon } from '../env-utils';
import { InlineNameCell } from './InlineNameCell';

/**
 * EnvSwitcher — top-of-section env picker.
 *
 * THIS IS THE ONLY PLACE AN ENVIRONMENT'S NAME APPEARS
 * ---------------------------------------------------
 * The env body used to open with its own header row — icon, then the name
 * as a click-to-edit cell — directly under this strip, so the active env
 * was spelled twice within a few centimetres of each other, and the module
 * layer (which shows the same environments) had no such row, because it had
 * nowhere to put rename. The strip already names every environment, so the
 * row went and the rename came up here with it: the **active** chip hosts the
 * `InlineNameCell`, which reads as the name with a pencil appearing on
 * hover. An inactive chip is a plain span — renaming something you cannot
 * see yet is not a gesture anyone needs.
 *
 * Because that puts a button inside what used to be a button, the chip is
 * now a `div` with `role="tab"`, `tabIndex` and hand-written Enter/Space
 * handling. Those are the semantics the old `<button>` gave for free; the
 * role and `aria-selected` are unchanged, so assistive technology sees the
 * same tab strip it always did.
 *
 * `showAdd` / `showDelete` exist for the **module** layer, which shows the
 * same environments but must not offer the two project-level writes: adding
 * an environment and deleting one both act on the project, and deleting
 * one from inside a module tab would take out every module's values for it.
 * The host points at the project's own 环境 layer instead. Default `true`,
 * so the settings section's behaviour is unchanged.
 */
export function EnvSwitcher({
  environments,
  activeId,
  onPick,
  onRequestDelete,
  onAdd,
  addDisabled,
  showAdd = true,
  showDelete = true,
  onRename,
  autoRenameId = null,
  onAutoRenameEnd,
}: {
  environments: Environment[];
  activeId: string | null;
  onPick: (id: string) => void;
  onRequestDelete: (env: Environment) => void;
  onAdd: () => void;
  /** Create is in flight — block a second submit until it resolves. */
  addDisabled?: boolean;
  /** Show the trailing 新建环境 button. */
  showAdd?: boolean;
  /** Show each env's delete affordance. */
  showDelete?: boolean;
  /**
   * Commit a new name for an environment. Omitted by the module layer,
   * which must not rename a project-level environment — that also turns the
   * active chip's name cell back into a plain span there.
   */
  onRename?: (envId: string, name: string) => void;
  /** Open this env's name cell already focused — set for a just-created one. */
  autoRenameId?: string | null;
  /** The auto-open was consumed; the owner clears its pending-rename flag. */
  onAutoRenameEnd?: () => void;
}) {
  const renameable = !!onRename;
  return (
    <div className="flex flex-wrap items-center gap-2">
      {environments.map((env) => {
        const isActive = env.id === activeId;
        const isRenaming = isActive && renameable;
        return (
          <div
            key={env.id}
            role="tab"
            aria-selected={isActive}
            tabIndex={0}
            // While the cell is open the chip stops being a switch: a click
            // that lands on the name (or anywhere else in the chip) must not
            // re-pick an environment the user is in the middle of naming.
            onClick={() => {
              if (!isRenaming) onPick(env.id);
            }}
            onKeyDown={(e: KeyboardEvent<HTMLDivElement>) => {
              // The open cell owns its own keys; Enter/Escape there commit
              // and cancel, and must not also reach the chip.
              if (isRenaming) return;
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                onPick(env.id);
              }
            }}
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
            {isRenaming ? (
              <InlineNameCell
                value={env.name}
                placeholder="环境名"
                autoEdit={autoRenameId === env.id}
                onAutoEditEnd={onAutoRenameEnd}
                onCommit={(name) => onRename?.(env.id, name)}
                className="font-mono text-xs font-medium"
              />
            ) : (
              <span className="font-mono font-medium">{env.name}</span>
            )}
            {showDelete && environments.length > 1 && (
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
          </div>
        );
      })}
      {showAdd && (
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
      )}
    </div>
  );
}
