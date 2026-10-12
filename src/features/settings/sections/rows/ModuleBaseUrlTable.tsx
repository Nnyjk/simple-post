import { FolderTree, Plus, Star, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Tooltip } from '@/components/ui/tooltip';
import type { BaseUrlDefinition, Environment, ResourceNode } from '@/types/domain';
import { HUE } from '@/lib/hue';
import { cn } from '@/lib/utils';
import type { EnvironmentSectionProps } from '../EnvironmentSection';
import { InlineNameCell } from './InlineNameCell';

/**
 * The 前置 URL table — one row per (module, service) pair.
 *
 * Shape is ApiFox's: three columns (模块 / 服务 / 前置 URL), modules
 * grouped in `sortOrder`, and the module name printed only on its group's
 * first row so the column reads as a rowspan.
 *
 * The row set is the **cartesian product** of the project's modules and
 * the project's services, not a join of the two: every module gets a row
 * for every service, because the service list is project-wide and every
 * module may legitimately point the same 服务 at a different host. That is
 * the whole reason `Environment.moduleBaseUrls` is keyed
 * `serviceId → moduleId` rather than flat.
 *
 * Consequently a cell holds one independent value and there is nothing to
 * inherit: an empty cell means "this module has no address for this
 * service here", full stop. It deliberately does **not** show another
 * module's cell or the project default's cell as a placeholder —
 * pre-filling it would be exactly the 偷偷借用 the model forbids.
 *
 * Services are listed in the order the project hands them over, and that
 * array order *is* the sort order: `db::base_url_defs::list_by_project`
 * already orders by `sort_order ASC, id ASC`, so the wire array arrives
 * sorted. Adding a `sortOrder` field to `BaseUrlDefinition` would
 * duplicate that ordering and let the two disagree, so the table renders
 * the array as-is and must not re-sort it.
 *
 * Presentational: everything arrives as props, including the module list
 * (already filtered to this project and sorted by the host).
 *
 * TWO MODES, ONE TABLE
 * --------------------
 * `mode="project"` is the whole-project view: the cartesian product, the
 * 模块 column, and full management of the *service definitions* (rename,
 * ★, delete, add) because that list is the project's own.
 *
 * `mode="module"` is the single-module view a **module tab** shows: the
 * row set collapses to just this module, the 模块 column disappears (there
 * is only one value in it), and the service cells turn read-only — the
 * service *list* belongs to the project, so renaming, starring or deleting
 * one from inside a module would be an action on something the user is not
 * looking at, and the delete would take away every other module's row for
 * it. What stays is the cell that is genuinely the module's own: its
 * address for each service, in this environment.
 */
interface ModuleBaseUrlTableProps {
  env: Environment;
  projectId: string;
  modules: readonly ResourceNode[];
  /** Which view this table is serving. See the note above. */
  mode?: 'project' | 'module';
  baseUrlDefinitions: readonly BaseUrlDefinition[];
  /** The project's ★ — one id for the whole project, not one per module. */
  defaultBaseUrlDefinitionId: string | null;
  setEnvironmentModuleBaseUrl: EnvironmentSectionProps['setEnvironmentModuleBaseUrl'];
  addBaseUrlDefinition: EnvironmentSectionProps['addBaseUrlDefinition'];
  updateBaseUrlDefinition: EnvironmentSectionProps['updateBaseUrlDefinition'];
  setDefaultBaseUrlDefinition: EnvironmentSectionProps['setDefaultBaseUrlDefinition'];
  onDeleteBaseUrl: (def: { id: string; name: string }) => void;
}

/** Column widths, shared by the header and every row so they line up. */
const GRID = 'grid grid-cols-[7rem_11rem_minmax(0,1fr)_auto] items-center gap-2';
/** Same table without the 模块 column — `mode="module"`. */
const GRID_SINGLE = 'grid grid-cols-[11rem_minmax(0,1fr)_auto] items-center gap-2';

export function ModuleBaseUrlTable({
  env,
  projectId,
  modules,
  mode = 'project',
  baseUrlDefinitions,
  defaultBaseUrlDefinitionId,
  setEnvironmentModuleBaseUrl,
  addBaseUrlDefinition,
  updateBaseUrlDefinition,
  setDefaultBaseUrlDefinition,
  onDeleteBaseUrl,
}: ModuleBaseUrlTableProps) {
  const single = mode === 'module';
  const grid = single ? GRID_SINGLE : GRID;

  if (modules.length === 0) {
    // The table's row set is (module × service), so with no module there
    // is nothing to configure — every cell would be unaddressable. Point
    // at the fix rather than rendering an empty grid.
    return (
      <div className="px-4 py-6 text-center text-[11px] text-muted-foreground">
        该项目还没有模块 · 先在左侧资源树新建模块，服务配置才有归属
      </div>
    );
  }

  if (baseUrlDefinitions.length === 0 && single) {
    // Project mode answers this with the 添加服务 row. Here that control
    // would be a project-level write hiding inside a module tab, and the
    // row set would be a single "add" line — so say where the service
    // list actually lives instead.
    return (
      <div className="px-4 py-6 text-center text-[11px] text-muted-foreground">
        该项目还没有定义服务 · 在项目标签里添加第一个服务
      </div>
    );
  }

  return (
    <div className="flex flex-col">
      <div className={cn(grid, 'border-b border-border/60 px-4 py-2')}>
        {!single && <span className="label-caps">模块</span>}
        <span className="label-caps">服务</span>
        <span className="label-caps">前置 URL</span>
        <span />
      </div>

      {modules.map((module, idx) => (
        <ModuleGroup
          key={module.id}
          env={env}
          projectId={projectId}
          module={module}
          // The full project list, not a per-module slice — that is the
          // cartesian product. In `module` mode `modules` is already the
          // single module, so the same expression is the right one.
          defs={baseUrlDefinitions}
          defaultBaseUrlDefinitionId={defaultBaseUrlDefinitionId}
          setEnvironmentModuleBaseUrl={setEnvironmentModuleBaseUrl}
          addBaseUrlDefinition={addBaseUrlDefinition}
          updateBaseUrlDefinition={updateBaseUrlDefinition}
          setDefaultBaseUrlDefinition={setDefaultBaseUrlDefinition}
          onDeleteBaseUrl={onDeleteBaseUrl}
          flush={idx === 0}
          single={single}
          grid={grid}
        />
      ))}
    </div>
  );
}

function ModuleGroup({
  env,
  projectId,
  module,
  defs,
  defaultBaseUrlDefinitionId,
  setEnvironmentModuleBaseUrl,
  addBaseUrlDefinition,
  updateBaseUrlDefinition,
  setDefaultBaseUrlDefinition,
  onDeleteBaseUrl,
  flush,
  single,
  grid,
}: {
  env: Environment;
  projectId: string;
  module: ResourceNode;
  defs: readonly BaseUrlDefinition[];
  defaultBaseUrlDefinitionId: string | null;
  setEnvironmentModuleBaseUrl: EnvironmentSectionProps['setEnvironmentModuleBaseUrl'];
  addBaseUrlDefinition: EnvironmentSectionProps['addBaseUrlDefinition'];
  updateBaseUrlDefinition: EnvironmentSectionProps['updateBaseUrlDefinition'];
  setDefaultBaseUrlDefinition: EnvironmentSectionProps['setDefaultBaseUrlDefinition'];
  onDeleteBaseUrl: (def: { id: string; name: string }) => void;
  flush: boolean;
  /** `mode="module"`: drop the 模块 column and the service controls. */
  single: boolean;
  /** The grid class for this mode, so header and rows cannot disagree. */
  grid: string;
}) {
  return (
    <div className="flex flex-col">
      {defs.map((def, i) => (
        <div
          key={`${module.id}:${def.id}`}
          className={cn(
            grid,
            'group px-4 py-1.5',
            !(flush && i === 0) && 'border-t border-border/60',
          )}
        >
          {/* Module cell — printed once per group. Empty cells below it
              are what turn this column into a visual rowspan. Omitted
              entirely in `module` mode, where the grid has no such
              column. */}
          {!single &&
            (i === 0 ? (
              <div className="flex min-w-0 items-center gap-1.5" title={module.name}>
                <FolderTree className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                <span className="truncate text-xs font-medium">{module.name}</span>
              </div>
            ) : (
              <span />
            ))}

          {/* Service cell — rename + project-wide default star. Both are
              project-level, so they repeat on every module's rows and
              editing either from any row edits the same thing. In
              `module` mode both are dropped: the pane is about this
              module's address, and the project owns the list those
              controls would edit. */}
          <div className="flex min-w-0 items-center gap-1">
            {single ? (
              <span className="truncate text-xs text-muted-foreground" title={def.name}>
                {def.name}
              </span>
            ) : (
              <>
                <InlineNameCell
                  value={def.name}
                  placeholder="服务名"
                  onCommit={(v) => updateBaseUrlDefinition(projectId, def.id, { name: v })}
                  className="text-xs"
                />
                <DefaultToggle
                  isDefault={def.id === defaultBaseUrlDefinitionId}
                  onToggle={() =>
                    setDefaultBaseUrlDefinition(
                      projectId,
                      def.id === defaultBaseUrlDefinitionId ? null : def.id,
                    )
                  }
                />
              </>
            )}
          </div>

          {/* URL cell — this module's own value for this service in this
              environment. Generic hint only: naming another cell here
              would promise an inheritance that does not exist.

              An emptied input sends `null`, not `''`. Both read as "no
              address" at send time, but only `null` actually *removes* the
              cell — the store deletes the key and prunes the service row
              once its last cell is gone, so a cleared project does not
              accumulate `defId → moduleId → ""` shells that read as
              "configured here" to anything diffing the object by key. */}
          <div className="min-w-0">
            <Input
              value={env.moduleBaseUrls[def.id]?.[module.id] ?? ''}
              onChange={(e) =>
                setEnvironmentModuleBaseUrl(
                  env.id,
                  def.id,
                  module.id,
                  e.target.value === '' ? null : e.target.value,
                )
              }
              placeholder="http:// 或 https:// 起始的前置 URL"
              aria-label={`${module.name} · ${def.name}`}
              className="h-8 font-mono text-xs"
              spellCheck={false}
            />
          </div>

          {/* Deleting removes the project-wide service, so it also takes
              away every other module's row for it. Hover-revealed, and
              every row carries one because the affordance belongs to the
              service rather than to any single cell. */}
          {!single && (
            <div className="flex shrink-0 items-center">
              <Tooltip content="删除服务" side="bottom">
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 text-muted-foreground opacity-0 hover:text-destructive group-hover:opacity-100"
                  onClick={() => onDeleteBaseUrl({ id: def.id, name: def.name })}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </Tooltip>
            </div>
          )}
        </div>
      ))}

      {/* Adding is offered per module group because that is the row the
          user is looking at when they add one — but the service itself
          lands on the project and shows up in every group. */}
      {!single && (
        <div
          className={cn(
            grid,
            'px-4 py-1.5',
            !(flush && defs.length === 0) && 'border-t border-border/60',
          )}
        >
          <span />
          <button
            type="button"
            onClick={() => addBaseUrlDefinition(projectId, '新服务')}
            className="flex items-center gap-1.5 text-[11px] text-muted-foreground transition-colors hover:text-foreground"
          >
            <Plus className="h-3.5 w-3.5" />
            添加服务
          </button>
          <span />
          <span />
        </div>
      )}
    </div>
  );
}

/**
 * The project's default service (★).
 *
 * Starred rows are what an endpoint resolves through when it names no
 * service itself, so this is the one control in the table that changes
 * the URL an existing interface sends. Clicking it off is not a no-op:
 * it deliberately leaves the project with no default, so every endpoint
 * that relied on one loses its baseUrl.
 */
function DefaultToggle({ isDefault, onToggle }: { isDefault: boolean; onToggle: () => void }) {
  return (
    <Tooltip content={isDefault ? '项目默认服务' : '设为项目默认'} side="top">
      <button
        type="button"
        onClick={onToggle}
        aria-pressed={isDefault}
        aria-label={isDefault ? '取消默认服务' : '设为默认服务'}
        className={cn(
          'inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md border transition-colors',
          isDefault
            ? cn(HUE.module.border, HUE.module.bg, HUE.module.text)
            : 'border-transparent text-muted-foreground/50 hover:bg-accent hover:text-foreground',
        )}
      >
        <Star className={cn('h-3 w-3', isDefault ? 'fill-current' : 'fill-transparent')} />
      </button>
    </Tooltip>
  );
}
