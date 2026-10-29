import { useEffect, useRef } from 'react';
import { ChevronRight } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { VarHintAnchor } from './VarHintAnchor';
import type { VarEntry } from './varHint';

/**
 * 候选浮层 —— 列出环境变量与动态变量，供插入 `{{key}}`。
 *
 * 纯展示的受控组件，父组件掌管：
 *  - 要显示的 `entries`（通常已按用户输入的半个变量名过滤）
 *  - 选中行下标及其变更方式（`onSelectedChange`）
 *  - 打开 / 关闭状态
 *
 * 位置锚在 `anchor`：落在 `top: 锚点底 + 4px`、`left: 锚点左`，宽度 >= 240px。
 */
export interface VarHintPickerProps {
  open: boolean;
  onClose: () => void;
  entries: VarEntry[];
  selected: number;
  onSelectedChange: (next: number) => void;
  onSelect: (key: string) => void;
  /** 定位浮层的锚点。 */
  anchor: VarHintAnchor;
  /** 头部标题，可选，如当前环境名。 */
  envName?: string;
}

function truncate(s: string | undefined, n: number): string {
  const str = s ?? '';
  return str.length <= n ? str : `${str.slice(0, n - 1)}…`;
}

export function VarHintPicker({
  open,
  onClose,
  entries,
  selected,
  onSelectedChange,
  onSelect,
  anchor,
  envName,
}: VarHintPickerProps) {
  const popRef = useRef<HTMLDivElement>(null);

  // 打开、窗口滚动或缩放时都重新定位。
  useEffect(() => {
    if (!open) return;
    const setPos = () => {
      const el = popRef.current;
      const rect = anchor.getRect();
      if (!el || !rect) return;
      el.style.left = `${rect.left}px`;
      el.style.top = `${rect.bottom + 4}px`;
      el.style.minWidth = `${Math.max(240, rect.right - rect.left)}px`;
    };
    setPos();
    window.addEventListener('resize', setPos);
    window.addEventListener('scroll', setPos, true);
    return () => {
      window.removeEventListener('resize', setPos);
      window.removeEventListener('scroll', setPos, true);
    };
  }, [open, anchor, entries.length]);

  // 点外部关闭，但点在锚点范围内不算。
  useEffect(() => {
    if (!open) return;
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (popRef.current?.contains(target)) return;
      if (anchor.contains(target)) return;
      onClose();
    };
    document.addEventListener('mousedown', onMouseDown);
    return () => document.removeEventListener('mousedown', onMouseDown);
  }, [open, onClose, anchor]);

  if (!open) return null;

  return (
    <div
      ref={popRef}
      role="listbox"
      aria-label="变量"
      data-testid="var-hint-picker"
      className="fixed z-50 max-h-64 overflow-y-auto rounded-md border border-border bg-popover text-popover-foreground shadow-lg animate-fade-in"
      onMouseDown={(e) => e.preventDefault() /* 保持输入框聚焦 */}
    >
      <div className="flex items-center gap-1.5 border-b border-border px-2 py-1 label-caps">
        <ChevronRight className="h-3 w-3" />
        插入变量{envName ? ` · ${envName}` : ''}
      </div>
      {entries.length === 0 && (
        <div className="px-3 py-6 text-center text-xs text-muted-foreground">没有匹配的变量</div>
      )}
      {entries.map((e, idx) => {
        const active = idx === selected;
        return (
          <button
            // key 里带上来源：环境变量与动态变量可能同名，两个同 key 的元素会让 React
            // 复用错行。
            key={`${e.origin ?? 'environment'}:${e.key}`}
            type="button"
            role="option"
            aria-selected={active}
            data-idx={idx}
            onMouseEnter={() => onSelectedChange(idx)}
            onClick={() => onSelect(e.key)}
            className={cn(
              'flex w-full items-center gap-2 px-2 py-1.5 text-left text-xs',
              active ? 'bg-primary/15 text-foreground' : 'hover:bg-accent/60',
            )}
          >
            <span className="shrink-0 font-mono text-primary">{`{{${e.key}}}`}</span>
            {e.origin && (
              <span className="shrink-0 rounded bg-muted px-1 text-[10px] text-muted-foreground">
                {e.origin === 'dynamic' ? '动态' : '环境'}
              </span>
            )}
            <span className="min-w-0 flex-1 truncate text-muted-foreground">
              {truncate(e.value, 60)}
            </span>
            {e.hint && (
              <span className="shrink-0 text-[10px] text-muted-foreground/80">{e.hint}</span>
            )}
          </button>
        );
      })}
      <div className="flex items-center gap-3 border-t border-border bg-card/40 px-2 py-1 text-[10px] text-muted-foreground">
        <span>↑↓ 选择</span>
        <span>Tab/Enter 插入</span>
        <span>Esc 关闭</span>
      </div>
    </div>
  );
}
