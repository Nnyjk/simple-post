import { useCallback, type MouseEvent as ReactMouseEvent, type RefObject } from 'react';
import { cn } from '@/lib/utils';

interface SplitterProps {
  direction: 'vertical' | 'horizontal';
  /** 父容器 ref，用于在 mousemove 中读取实际位置和限制 */
  containerRef: RefObject<HTMLElement>;
  /** 当前上半/左半的 flex-basis 字符串，如 "50%" */
  basis: string;
  /** 拖动时持续调用，参数是新的 flex-basis 字符串 */
  onResize: (basis: string) => void;
  /** 双击复位（可选） */
  onReset?: () => void;
  /** 上半部分的最小高度（px），默认 120 */
  minSize?: number;
  /** 下半部分的最小高度（px），默认 120 */
  minOtherSize?: number;
}

function parseBasisPx(basis: string, containerSize: number): number {
  // basis can be a percentage like "50%" or a pixel value. We always store
  // percentages in App.tsx, but be defensive in case a caller passes "500px".
  const trimmed = basis.trim();
  if (trimmed.endsWith('%')) {
    const pct = parseFloat(trimmed.slice(0, -1));
    if (Number.isNaN(pct)) return containerSize / 2;
    return (pct / 100) * containerSize;
  }
  if (trimmed.endsWith('px')) {
    const px = parseFloat(trimmed.slice(0, -2));
    return Number.isNaN(px) ? containerSize / 2 : px;
  }
  const n = parseFloat(trimmed);
  return Number.isNaN(n) ? containerSize / 2 : n;
}

function getContainerSize(el: HTMLElement, direction: 'vertical' | 'horizontal'): number {
  const rect = el.getBoundingClientRect();
  return direction === 'horizontal' ? rect.height : rect.width;
}

export function Splitter({
  direction,
  containerRef,
  basis,
  onResize,
  onReset,
  minSize = 120,
  minOtherSize = 120,
}: SplitterProps) {
  const handleMouseDown = useCallback(
    (e: ReactMouseEvent<HTMLDivElement>) => {
      e.preventDefault();
      e.stopPropagation();

      const container = containerRef.current;
      if (!container) return;

      const containerSize = getContainerSize(container, direction);
      if (containerSize <= 0) return;

      const startAxis = direction === 'horizontal' ? e.clientY : e.clientX;
      const startBasisPx = parseBasisPx(basis, containerSize);

      const previousCursor = document.body.style.cursor;
      const previousUserSelect = document.body.style.userSelect;
      document.body.style.cursor = direction === 'horizontal' ? 'row-resize' : 'col-resize';
      document.body.style.userSelect = 'none';

      const onMove = (ev: MouseEvent) => {
        const currentContainerSize = getContainerSize(container, direction);
        if (currentContainerSize <= 0) return;

        const axisNow = direction === 'horizontal' ? ev.clientY : ev.clientX;
        const delta = axisNow - startAxis;
        const maxBasis = currentContainerSize - minOtherSize;
        const lower = Math.min(minSize, maxBasis);
        const upper = Math.max(maxBasis, minSize);
        const newPx = Math.min(Math.max(startBasisPx + delta, lower), upper);
        const pct = (newPx / currentContainerSize) * 100;
        onResize(`${pct.toFixed(2)}%`);
      };

      const onUp = () => {
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);
        document.body.style.cursor = previousCursor;
        document.body.style.userSelect = previousUserSelect;
      };

      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', onUp);
    },
    [basis, containerRef, direction, minOtherSize, minSize, onResize],
  );

  const handleDoubleClick = useCallback(
    (e: ReactMouseEvent<HTMLDivElement>) => {
      e.preventDefault();
      e.stopPropagation();
      onReset?.();
    },
    [onReset],
  );

  const isHorizontal = direction === 'horizontal';

  return (
    <div
      role="separator"
      aria-orientation={isHorizontal ? 'horizontal' : 'vertical'}
      data-testid="splitter"
      onMouseDown={handleMouseDown}
      onDoubleClick={handleDoubleClick}
      className={cn(
        'group shrink-0 bg-border transition-colors',
        isHorizontal
          ? 'h-1.5 w-full cursor-row-resize hover:bg-primary/30 active:bg-primary/50'
          : 'h-full w-1.5 cursor-col-resize hover:bg-primary/30 active:bg-primary/50',
      )}
    >
      <div
        className={cn(
          'flex items-center justify-center',
          isHorizontal ? 'h-full w-full' : 'h-full w-full',
        )}
      >
        <span className="flex h-3 flex-col items-center justify-center gap-0.5">
          <span className="h-0.5 w-0.5 rounded-full bg-muted-foreground/60" />
          <span className="h-0.5 w-0.5 rounded-full bg-muted-foreground/60" />
          <span className="h-0.5 w-0.5 rounded-full bg-muted-foreground/60" />
        </span>
      </div>
    </div>
  );
}
