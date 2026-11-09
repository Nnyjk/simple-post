import { FolderTree } from 'lucide-react';
import { Input } from '@/components/ui/input';
import type { Environment, ResourceNode } from '@/types/domain';
import { cn } from '@/lib/utils';
import type { EnvironmentSectionProps } from '../EnvironmentSection';

/**
 * 模块地址表 —— 每行一个模块，三列：模块、模块地址与路径改写。一行是一个 (环境 × 模块) 格子，
 * 格子里有两个字段：
 *
 * - **模块地址** —— 文档里写的、模块对外声明的那个地址。
 * - **路径改写** —— 一对「起点 → 目标」。它把端点**自己 URL** 的路径前缀从起点换成目标，
 *   模块地址里的主机与路径段一概不动。故模块地址是 `127.0.0.1:8080`、起点是 `/api/air`、
 *   目标是 `/api/project-a/air` 时，端点路径 `/api/air/xxxxx` 发出的是
 *   `127.0.0.1:8080/api/project-a/air/xxxxx`。
 *
 * 两个字段各自独立写入：`null` = 该字段保持原值；起点与目标都空白的规则 = 清空规则。两个字段
 * 都空时这一格整个消失。每个格子都是独立值，没有任何东西可继承。
 *
 * 纯展示：一切都由 props 传入，包括模块列表（已按项目过滤并由宿主排序）。
 *
 * 两种模式，一张表：`mode="project"` 是全项目视图，行集是本项目的全部模块，模块 列出现；
 * `mode="module"` 是**模块标签**所示的单模块视图，行集收缩为仅此模块，模块 列消失。
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
/** 路径改写那一列里的两个输入框并排。 */
const REWRITE_GRID = 'grid grid-cols-2 items-center gap-1';

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
        <span className="label-caps" title="把端点自己 URL 的路径前缀从起点换成目标">
          路径改写
        </span>
      </div>

      {modules.map((module) => {
        const cell = env.moduleBaseUrls[module.id];
        const rewrite = cell?.rewrite ?? null;
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

            {/* 模块地址 —— 本模块在此环境下的地址。只给通用提示：在此点名另一个
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

            {/* 路径改写 —— 一对「起点 → 目标」。起点按原文匹配端点自己 URL 的开头，命中要求落在
                路径边界上；目标可以留空（把这段前缀整个去掉）。两个输入框各自写入自己那一半，
                另一半保持原值。 */}
            <div className={cn(REWRITE_GRID, 'min-w-0')}>
              <Input
                value={rewrite?.from ?? ''}
                onChange={(e) =>
                  setEnvironmentModuleBaseUrl(env.id, module.id, null, {
                    from: e.target.value,
                    to: rewrite?.to ?? '',
                  })
                }
                placeholder="起点 /api/air"
                aria-label={`${module.name} · 路径改写起点`}
                className="h-8 font-mono text-xs"
                spellCheck={false}
              />
              <Input
                value={rewrite?.to ?? ''}
                onChange={(e) =>
                  setEnvironmentModuleBaseUrl(env.id, module.id, null, {
                    from: rewrite?.from ?? '',
                    to: e.target.value,
                  })
                }
                placeholder="目标 /api/project-a/air"
                aria-label={`${module.name} · 路径改写目标`}
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
