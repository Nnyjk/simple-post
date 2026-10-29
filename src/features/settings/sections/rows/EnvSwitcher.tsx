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
 * EnvSwitcher —— 分区顶部的环境选择条。
 *
 * 环境名只在这里出现一次
 * ----------------------
 * 这条选择条已经叫出了每一个环境的名字，故**激活**的那枚 chip 自带 `InlineNameCell`，
 * 读起来就是名字加一个悬停才出现的铅笔。未激活的 chip 只是普通 span —— 去改一个还看不见
 * 的环境不是谁需要的动作。
 *
 * 由此 chip 内部出现了按钮，故它改成带 `role="tab"`、`tabIndex` 与手写 Enter / Space
 * 的 `div`：`<button>` 自带的按钮语义由 `role="tab"`、`tabIndex` 与这段按键处理补齐。
 * `role` 与 `aria-selected` 未变，辅助技术看到的仍是同一条标签条。
 *
 * `showAdd` / `showDelete` 是为**模块**层准备的：它展示同一批环境，但不能提供那两个
 * 项目级写入 —— 新建环境和删除环境都作用在项目上，而在模块标签里删掉一个环境会连带
 * 抹掉每个模块在它下面的值。那一层改为指向项目自己的环境分区。默认 `true`，
 * 设置分区的行为不变。
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
  /** 新建进行中 —— 挡住第二次提交，直到新建请求结束。 */
  addDisabled?: boolean;
  /** 显示末尾的新建环境按钮。 */
  showAdd?: boolean;
  /** 显示每个环境的删除入口。 */
  showDelete?: boolean;
  /**
   * 为某个环境提交新名字。模块层不传：它不该重命名项目级的环境，也因此那里的激活 chip
   * 名字单元格退回普通 span。
   */
  onRename?: (envId: string, name: string) => void;
  /** 直接以聚焦状态打开该环境的名称单元格 —— 用于刚建出来的那个。 */
  autoRenameId?: string | null;
  /** 自动打开已被消费；属主据此清掉自己的待重命名标志。 */
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
            // 单元格打开期间 chip 不再是切换器：落在名字上（或 chip 内任何位置）的点击
            // 不该把用户正在命名的环境切走。
            onClick={() => {
              if (!isRenaming) onPick(env.id);
            }}
            onKeyDown={(e: KeyboardEvent<HTMLDivElement>) => {
              // 打开的单元格自己管按键：在那里 Enter / Esc 分别是提交与取消，
              // 不该同时传到 chip。
              if (isRenaming) return;
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                onPick(env.id);
              }
            }}
            className={cn(
              'group relative flex items-center gap-2 rounded-lg border px-2.5 py-1.5 text-xs transition-colors',
              // chip 的着色对应项目的档位语言（天蓝 = 基础 / 数据，琥珀 = 配置 / 构建，
              // 紫 = 类生产）。`envIcon` 按环境名自动挑，用户不必逐个打标。
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
