import { FolderTree } from 'lucide-react';
import { Input } from '@/components/ui/input';
import type { Environment, ResourceNode } from '@/types/domain';
import { cn } from '@/lib/utils';
import type { EnvironmentSectionProps } from '../EnvironmentSection';

/**
 * 模块地址表 —— 每行一个模块，两列：模块地址与代理地址。
 *
 * 一行是一个 (环境 × 模块) 格子，格子里有两个字段：
 *
 *   - **模块地址** —— 文档里写的、模块对外声明的那个地址。
 *   - **代理地址** —— 接口经代理网关转代后真正要打的那个地址。留空即没有代理。
 *
 * 两个字段各自独立写入：`null` = 该字段保持原值，`''` = 清空该字段。两个字段都空
 * 时这一格整个消失。
 *
 * 每个格子都是独立值，没有任何东西可继承：空格就是「此模块在此环境没有地址」，
 * 仅此而已。它刻意**不**把别的模块的格子显示成占位 —— 该模型不允许预填别的格子。
 *
 * 纯展示：一切都由 props 传入，包括模块列表（已按项目过滤并由宿主排序）。
 *
 * 两种模式，一张表
 * --------------------
 * `mode="project"` 是全项目视图：行集是本项目的全部模块，模块 列出现。
 *
 * `mode="module"` 是**模块标签**所示的单模块视图：行集收缩为仅此模块，模块 列消失
 * （其中只有一个值，印出来只是重复）。留下的是真正属于该模块的东西：它在此环境下
 * 的地址与代理地址。
 */
interface ModuleBaseUrlTableProps {
  env: Environment;
  modules: readonly ResourceNode[];
  /** 本表服务的是哪个视图，见上方说明。 */
  mode?: 'project' | 'module';
  setEnvironmentModuleBaseUrl: EnvironmentSectionProps['setEnvironmentModuleBaseUrl'];
}

/** 列宽，由表头与每一行共用以保证对齐。 */
const GRID = 'grid grid-cols-[7rem_minmax(0,1fr)_minmax(0,1fr)] items-center gap-2';
/** 同一张表去掉 模块 列 —— `mode="module"`。 */
const GRID_SINGLE = 'grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] items-center gap-2';

export function ModuleBaseUrlTable({
  env,
  modules,
  mode = 'project',
  setEnvironmentModuleBaseUrl,
}: ModuleBaseUrlTableProps) {
  const single = mode === 'module';
  const grid = single ? GRID_SINGLE : GRID;

  if (modules.length === 0) {
    // 行集是 (环境 × 模块)，没有模块就无从配置 —— 每个格子都无法寻址。故直接指出修法，
    // 而不是渲染一张空网格。
    return (
      <div className="px-4 py-6 text-center text-[11px] text-muted-foreground">
        该项目还没有模块 · 先在左侧资源树新建模块，地址才有归属
      </div>
    );
  }

  return (
    <div className="flex flex-col">
      <div className={cn(grid, 'border-b border-border/60 px-4 py-2')}>
        {!single && <span className="label-caps">模块</span>}
        <span className="label-caps">模块地址</span>
        <span className="label-caps">代理地址</span>
      </div>

      {modules.map((module) => {
        const cell = env.moduleBaseUrls[module.id];
        return (
          <div
            key={module.id}
            className={cn(grid, 'group border-t border-border/60 px-4 py-1.5 first:border-t-0')}
          >
            {/* 模块单元格 —— 模块名与图标。`module` 模式下整个省略，那里网格没有这一列。 */}
            {!single && (
              <div className="flex min-w-0 items-center gap-1.5" title={module.name}>
                <FolderTree className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                <span className="truncate text-xs font-medium">{module.name}</span>
              </div>
            )}

            {/* 模块地址 —— 本模块在此环境下的后端 api 地址。只给通用提示：在此点名另一个
                格子等于承诺一种并不存在的继承。

                输入框送空串即清空该字段；另一个字段送 `null` 表示保持原值。 */}
            <div className="min-w-0">
              <Input
                value={cell?.url ?? ''}
                onChange={(e) =>
                  setEnvironmentModuleBaseUrl(env.id, module.id, e.target.value, null)
                }
                placeholder="http:// 或 https:// 起始的地址"
                aria-label={`${module.name} · 模块地址`}
                className="h-8 font-mono text-xs"
                spellCheck={false}
              />
            </div>

            {/* 代理地址 —— 实际访问地址。留空即没有代理，请求直接打模块地址。 */}
            <div className="min-w-0">
              <Input
                value={cell?.proxy ?? ''}
                onChange={(e) =>
                  setEnvironmentModuleBaseUrl(env.id, module.id, null, e.target.value)
                }
                placeholder="留空即没有代理"
                aria-label={`${module.name} · 代理地址`}
                className="h-8 font-mono text-xs"
                spellCheck={false}
              />
            </div>
          </div>
        );
      })}
    </div>
  );
}
