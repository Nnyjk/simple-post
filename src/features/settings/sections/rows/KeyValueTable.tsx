import type { ReactNode } from 'react';
import { Plus } from 'lucide-react';
import { IconTile } from '@/components/ui/icon-tile';
import type { HueConcept } from '@/lib/hue';
import { cn } from '@/lib/utils';
/**
 * KeyValueTable —— 键值行表格的外壳：每一行的描边色块、行内边距与末尾的「添加」按钮。
 *
 * `KeyValueTable` 只画外壳，行内的单元格由调用方的 `renderRow` 提供。环境面板的
 * 「环境变量」表（`VariableTable`）用它；模块地址表（`ModuleBaseUrlTable`）自己画
 * 网格，不用它。
 *
 * 「添加」按钮在两种状态下是同一个元素：有内容的行列表与空状态渲染的是同一个按钮，
 * 故两种状态之间不会漂移出两样东西。
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
  /** 行内边距。调用方用它微调每一行的高度。 */
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
  // 「添加」按钮在两种状态下是同一个元素。把它提到分支之外：两个分支各写一份，
  // 空状态才会长得和有内容的状态不一样。
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

/** 每行开头的那块 36px 描边色块。`RowTile` 渲染它，各表自己决定图标与语义色相。 */
export function RowTile({ concept, icon }: { concept: HueConcept; icon: ReactNode }) {
  return <IconTile size="lg" bordered concept={concept} icon={icon} />;
}
