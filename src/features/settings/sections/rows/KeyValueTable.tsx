import type { ReactNode } from 'react';
import { Plus } from 'lucide-react';
import { IconTile } from '@/components/ui/icon-tile';
import type { HueConcept } from '@/lib/hue';
import { cn } from '@/lib/utils';
/**
 * KeyValueTable —— 环境面板两张行表（前置 URL 定义、环境变量）共用的外壳。
 *
 * 两者只有这些不同：图标、强调色、标签、变量键的校验规则，以及前置 URL 额外的
 * 「设为默认」开关。这份公共外壳住在这里，各表自己的单元格由 `renderRow` 提供。
 *
 * 新增控件是两种状态共用的同一个元素。空状态只保留提示文案，渲染的正是同一个页脚按钮，
 * 故两个状态之间不会漂移出两样东西。
 */

export interface KeyValueTableProps<T> {
  items: readonly T[];
  /** 稳定的 React key。必填，因为 `env.variables` 允许空键。 */
  keyOf: (item: T, index: number) => string;
  /** `items` 为空时居中显示。 */
  emptyText: string;
  /** 空状态按钮与页脚按钮共用的标签。 */
  addText: string;
  onAdd: () => void;
  /** 行内边距。前置 URL 的行比变量行略高。 */
  rowClassName?: string;
  /**
   * 行的单元格内容，不含行外壳。收到 `flush` 参数，行据此在自己是第一行时抑制上边框。
   */
  renderRow: (item: T, index: number, flush: boolean) => ReactNode;
}

export function KeyValueTable<T>({
  items,
  keyOf,
  emptyText,
  addText,
  onAdd,
  rowClassName,
  renderRow,
}: KeyValueTableProps<T>) {
  // 一个控件，两种状态。把它提到分支之外正是要点：每个分支各写一份，空状态才会
  // 长得和有内容的状态不一样。
  const addControl = (
    <button
      type="button"
      onClick={onAdd}
      className="flex items-center gap-2 border-t border-border/60 px-4 py-2 text-[11px] text-muted-foreground transition-colors hover:bg-accent/40 hover:text-foreground"
    >
      <Plus className="h-3.5 w-3.5" />
      {addText}
    </button>
  );

  if (items.length === 0) {
    return (
      <div className="flex flex-col">
        <div className="px-4 py-6 text-center text-[11px] text-muted-foreground">{emptyText}</div>
        {addControl}
      </div>
    );
  }

  return (
    <div className="flex flex-col">
      {items.map((item, idx) => {
        const flush = idx === 0;
        return (
          <div
            key={keyOf(item, idx)}
            className={cn(
              'group flex items-start gap-3 px-4',
              rowClassName,
              !flush && 'border-t border-border/60',
            )}
          >
            {renderRow(item, idx, flush)}
          </div>
        );
      })}
      {addControl}
    </div>
  );
}

/** 两张表都放在每行开头的那块 36px 描边色块。两表的 `py-*` 微调不同，故留作参数。 */
export function RowTile({ concept, icon }: { concept: HueConcept; icon: ReactNode }) {
  return <IconTile size="lg" bordered concept={concept} icon={icon} />;
}
