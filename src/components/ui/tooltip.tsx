import { type ReactNode, useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { cn } from '@/lib/utils';

interface TooltipProps {
  content: ReactNode;
  children: ReactNode;
  side?: 'top' | 'bottom';
  className?: string;
}

/** 触发器与气泡之间的间距（px）。 */
const GAP = 6;
/** 贴边收拢时与视口边缘保持的距离（px）。 */
const EDGE = 8;

/**
 * 悬浮提示，portal 到 `document.body` 并用 `fixed` 定位。
 *
 * portal 是要点。气泡若作为触发器的绝对定位子元素，就会被布局进触发器所在的容器
 * 并被它裁掉：节点树里那个容器是 `overflow-y-auto`，于是靠近顶部的行提示会被滚动
 * 框切掉，靠近底部的行则被面板边缘吃掉。`z-index` 修不了裁剪——无论层叠顺序如何，
 * 祖先的 `overflow` 都赢。逃逸到 `document.body` 并量触发器的 rect 才是真正让提示
 * 可读的办法，这也正是 `TreeContextMenu` 与 `Dialog` 已有的形状。
 *
 * 气泡在挂载后测量，并在滚动与 resize 时重新测量，好让它在树于静止指针下方滚动时
 * 始终贴着触发器。
 */
export function Tooltip({ content, children, side = 'top', className }: TooltipProps) {
  const [visible, setVisible] = useState(false);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);
  const anchorRef = useRef<HTMLSpanElement>(null);
  const bubbleRef = useRef<HTMLSpanElement>(null);
  const timeoutRef = useRef<number | null>(null);

  // `content` 为 nullish 是有意的「这里不提示」——响应查看器的格式化按钮在 body
  // 是合法 JSON 时就会传一个空值。仍然渲染气泡只会在屏幕上多一枚空药丸。
  const hasContent = content !== undefined && content !== null && content !== '';

  const cancelPending = useCallback(() => {
    if (timeoutRef.current) window.clearTimeout(timeoutRef.current);
    timeoutRef.current = null;
  }, []);

  useEffect(() => cancelPending, [cancelPending]);

  const place = useCallback(() => {
    const anchor = anchorRef.current;
    const bubble = bubbleRef.current;
    if (!anchor || !bubble) return;
    const a = anchor.getBoundingClientRect();
    const b = bubble.getBoundingClientRect();
    let top = side === 'top' ? a.top - b.height - GAP : a.bottom + GAP;
    // 与其开到屏幕外，不如翻到另一侧——一个没处可去的提示比位置不对的更糟。
    if (side === 'top' && top < EDGE) top = a.bottom + GAP;
    else if (side === 'bottom' && top + b.height > window.innerHeight - EDGE) {
      top = a.top - b.height - GAP;
    }
    let left = a.left + a.width / 2 - b.width / 2;
    // 树是左侧一块窄面板，所以靠近任一边缘的行，居中的气泡没有这一步就会跑出窗口
    left = Math.min(Math.max(left, EDGE), Math.max(EDGE, window.innerWidth - b.width - EDGE));
    setPos({ top, left });
  }, [side]);

  // 用 `useLayoutEffect` 而非 `useEffect`：首帧之前就量好，气泡才不会在测量前的原点
  // 闪一下
  useLayoutEffect(() => {
    if (!visible) return;
    place();
    // 捕获阶段——树的滚动框是祖先而非 window 本身，冒泡阶段的监听器永远不触发。
    window.addEventListener('scroll', place, true);
    window.addEventListener('resize', place);
    return () => {
      window.removeEventListener('scroll', place, true);
      window.removeEventListener('resize', place);
    };
  }, [visible, place]);

  const handleEnter = () => {
    if (!hasContent) return;
    cancelPending();
    timeoutRef.current = window.setTimeout(() => setVisible(true), 250);
  };

  const handleLeave = () => {
    cancelPending();
    setVisible(false);
    setPos(null);
  };

  return (
    <span
      ref={anchorRef}
      className="relative inline-flex"
      onMouseEnter={handleEnter}
      onMouseLeave={handleLeave}
    >
      {children}
      {visible &&
        hasContent &&
        typeof document !== 'undefined' &&
        createPortal(
          <span
            ref={bubbleRef}
            role="tooltip"
            // 挂载后测量；rect 已知之前保持隐藏。
            style={{
              top: pos?.top ?? 0,
              left: pos?.left ?? 0,
              visibility: pos ? 'visible' : 'hidden',
            }}
            className={cn(
              'pointer-events-none fixed z-[150] max-w-[min(24rem,calc(100vw-1rem))] whitespace-pre-wrap rounded-md border border-border bg-popover px-2 py-1 text-xs text-popover-foreground shadow-md animate-fade-in',
              className,
            )}
          >
            {content}
          </span>,
          document.body,
        )}
    </span>
  );
}
