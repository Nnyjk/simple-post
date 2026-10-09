import { type ReactNode, useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { cn } from '@/lib/utils';

interface TooltipProps {
  content: ReactNode;
  children: ReactNode;
  side?: 'top' | 'bottom';
  className?: string;
}

/** Gap between the trigger and the bubble, in px. */
const GAP = 6;
/** Distance kept from the viewport edge when clamping, in px. */
const EDGE = 8;

/**
 * Hover hint, portaled to `document.body` and positioned `fixed`.
 *
 * The portal is the point. The bubble used to be an absolutely
 * positioned child of its trigger, which meant it was laid out inside
 * whatever the trigger lived in — and clipped by it. In the node tree
 * that container is `overflow-y-auto`, so a row near the top had its
 * "新建" hint sliced off by the scroll box, and a row near the bottom
 * lost it to the panel edge. `z-index` cannot fix a clip: an ancestor's
 * `overflow` wins regardless of stacking order. Escaping to `document.body`
 * and measuring the trigger's rect is what actually makes the hint
 * readable, and it is the same shape `TreeContextMenu` and `Dialog`
 * already use.
 *
 * The bubble is measured after mount, then re-measured on scroll and
 * resize so it stays glued to its trigger while the tree scrolls
 * underneath a stationary pointer.
 */
export function Tooltip({ content, children, side = 'top', className }: TooltipProps) {
  const [visible, setVisible] = useState(false);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);
  const anchorRef = useRef<HTMLSpanElement>(null);
  const bubbleRef = useRef<HTMLSpanElement>(null);
  const timeoutRef = useRef<number | null>(null);

  // A nullish `content` is a deliberate "no hint here" — the response
  // viewer's format button passes one when the body is valid JSON.
  // Rendering the bubble anyway put an empty pill on screen.
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
    // Flip to the opposite side rather than open off-screen — a
    // tooltip with nowhere to go is worse than one in the wrong place.
    if (side === 'top' && top < EDGE) top = a.bottom + GAP;
    else if (side === 'bottom' && top + b.height > window.innerHeight - EDGE) {
      top = a.top - b.height - GAP;
    }
    let left = a.left + a.width / 2 - b.width / 2;
    // The tree is a narrow left-hand panel, so a centred bubble on a
    // row near either edge runs off the window without this.
    left = Math.min(Math.max(left, EDGE), Math.max(EDGE, window.innerWidth - b.width - EDGE));
    setPos({ top, left });
  }, [side]);

  // `useLayoutEffect`, not `useEffect`: measure before the first paint
  // so the bubble never flashes at the pre-measurement origin.
  useLayoutEffect(() => {
    if (!visible) return;
    place();
    // Capture phase — the tree's scroll box is an ancestor, not the
    // window itself, so a bubble-phase listener would never fire.
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
            // Measured after mount; hidden until the rect is known.
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
