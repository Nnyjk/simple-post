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
 * 前置 URL 表 —— 每行一个 (模块, 服务) 对。
 *
 * 形状取自 ApiFox：三列（模块 / 服务 / 前置 URL），模块按 `sortOrder` 分组，模块名只印在
 * 每组首行，使该列读起来像 rowspan。
 *
 * 行集是项目模块与项目服务的**笛卡尔积**，不是两者的 join：每个模块为每个服务各得一行，
 * 因为服务列表是项目级的，而每个模块都可能合理地把同一个 服务 指向不同 host。这正是
 * `Environment.moduleBaseUrls` 按 `serviceId → moduleId` 作键而非扁平结构的原因。
 *
 * 因此单元格各持一个独立值，没有任何东西可继承：空单元格就是「此模块在此环境对此服务没有
 * 地址」，仅此而已。它刻意**不**把别的模块或项目默认的单元格显示成占位 —— 预填正是该
 * 模型所禁止的 偷偷借用。
 *
 * 服务按项目给出的顺序列出，而这个数组顺序*就是*排序：
 * `db::base_url_defs::list_by_project` 已按 `sort_order ASC, id ASC` 排序，故线上数组
 * 到达时已排好。给 `BaseUrlDefinition` 加一个 `sortOrder` 字段会重复这份排序并让两者
 * 分歧，故本表按原样渲染、不得重排。
 *
 * 纯展示：一切都由 props 传入，包括模块列表（已按项目过滤并由宿主排序）。
 *
 * 两种模式，一张表
 * --------------------
 * `mode="project"` 是全项目视图：笛卡尔积、模块 列，以及对*服务定义*的完整管理
 * （重命名、★、删除、新增），因为那个列表属于项目本身。
 *
 * `mode="module"` 是**模块标签**所示的单模块视图：行集收缩为仅此模块，模块 列消失
 * （其中只有一个值），服务单元格变只读 —— 服务*列表*属于项目，从模块内部重命名、加星或
 * 删除一个服务，就是对用户正在看的东西之外的对象动手，而且删除会连带拿走其他所有模块
 * 为它的那一行。留下的是真正属于该模块的东西：它在此环境下对每个服务的地址。
 */
interface ModuleBaseUrlTableProps {
  env: Environment;
  projectId: string;
  modules: readonly ResourceNode[];
  /** 本表服务的是哪个视图，见上方说明。 */
  mode?: 'project' | 'module';
  baseUrlDefinitions: readonly BaseUrlDefinition[];
  /** 项目 ★ —— 整个项目一个 id，不是每个模块一个。 */
  defaultBaseUrlDefinitionId: string | null;
  setEnvironmentModuleBaseUrl: EnvironmentSectionProps['setEnvironmentModuleBaseUrl'];
  addBaseUrlDefinition: EnvironmentSectionProps['addBaseUrlDefinition'];
  updateBaseUrlDefinition: EnvironmentSectionProps['updateBaseUrlDefinition'];
  setDefaultBaseUrlDefinition: EnvironmentSectionProps['setDefaultBaseUrlDefinition'];
  onDeleteBaseUrl: (def: { id: string; name: string }) => void;
}

/** 列宽，由表头与每一行共用以保证对齐。 */
const GRID = 'grid grid-cols-[7rem_11rem_minmax(0,1fr)_auto] items-center gap-2';
/** 同一张表去掉 模块 列 —— `mode="module"`。 */
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
    // 行集是 (模块 × 服务)，没有模块就无从配置 —— 每个单元格都无法寻址。故直接指出修法，
    // 而不是渲染一张空网格。
    return (
      <div className="px-4 py-6 text-center text-[11px] text-muted-foreground">
        该项目还没有模块 · 先在左侧资源树新建模块，服务配置才有归属
      </div>
    );
  }

  if (baseUrlDefinitions.length === 0 && single) {
    // 项目模式用「添加服务」那一行作答。此处那个控件会是藏在模块标签里的项目级写入，且行集
    // 会只剩一行「新增」—— 故改为说明服务列表实际在哪里。
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
          // 完整项目列表而非每模块的切片 —— 这就是笛卡尔积。`module` 模式下 `modules` 本就
          // 是那唯一一个模块，故同一个表达式在两种模式下都对。
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
  /** `mode="module"`：去掉 模块 列与服务控件。 */
  single: boolean;
  /** 本模式的网格 class，保证表头与各行不会分歧。 */
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
          {/* 模块单元格 —— 每组只印一次。它下面的空单元格让这一列读起来像 rowspan。
              `module` 模式下整个省略，那里网格没有这一列。 */}
          {!single &&
            (i === 0 ? (
              <div className="flex min-w-0 items-center gap-1.5" title={module.name}>
                <FolderTree className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                <span className="truncate text-xs font-medium">{module.name}</span>
              </div>
            ) : (
              <span />
            ))}

          {/* 服务单元格 —— 重命名 + 项目级默认星标。两者都是项目级的，故在每个模块的行上
              重复出现，从任意一行改的都是同一个东西。`module` 模式下两者都去掉：该面板讲的
              是本模块的地址，而这些控件要改的列表归项目所有。 */}
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

          {/* URL 单元格 —— 本模块在此环境下对此服务的自有值。只给通用提示：在此点名另一个
              单元格等于承诺一种并不存在的继承。

              清空的输入框送的是 `null` 而非 `''`。两者在发送时都读作「无地址」，但只有
              `null` 真正**删除**该单元格 —— store 会删掉这个键，并在最后一个单元格消失时
              剪掉服务行，故清空过的项目不会堆积 `defId → moduleId → ""` 的空壳，让任何
              按键 diff 对象的读法误判为「此处已配置」。 */}
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

          {/* 删除的是项目级的服务，故会连带拿走其他所有模块为它的那一行。悬停才显示，且
              每行都带一个 —— 这个便利性属于服务，而不是任何单个单元格。 */}
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

      {/* 「添加服务」按模块组给出，因为新增时用户正看着那一行 —— 但服务本身落在项目上，
          会在每个组里都出现。 */}
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
 * 项目默认服务（★）。
 *
 * 接口自身未指定服务时按带星的那一行解析，故这是表中唯一会改变**既有**接口所发 URL 的
 * 控件。把它点掉不是空操作：它刻意让项目落到没有默认，于是每个依赖默认的接口都失去它
 * 的 baseUrl。
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
