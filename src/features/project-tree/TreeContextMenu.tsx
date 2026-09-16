import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { cn } from '@/lib/utils';

export type ContextMenuItem =
  | {
      kind: 'item';
      label: string;
      icon?: ReactNode;
      onClick: () => void;
      /** Highlight in red (e.g. delete). */
      danger?: boolean;
      /** Render as non-interactive (greyed out). */
      disabled?: boolean;
    }
  | { kind: 'divider' };

interface TreeContextMenuProps {
  open: boolean;
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
}

/**
 * Lightweight right-click menu, portaled to `document.body`.
 *
 * - Auto-flips to stay within the viewport (clamps to right / bottom edge).
 * - Closes on outside mousedown, Escape, or selecting any item.
 * - The browser's native context menu is suppressed on the menu root.
 */
export function TreeContextMenu({ open, x, y, items, onClose }: TreeContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });

  // After mount, adjust position so the menu stays inside the viewport.
  useLayoutEffect(() => {
    if (!open) return;
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let left = x;
    let top = y;
    if (left + rect.width > vw - 8) left = Math.max(8, vw - rect.width - 8);
    if (top + rect.height > vh - 8) top = Math.max(8, vh - rect.height - 8);
    setPos({ left, top });
  }, [open, x, y, items.length]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onClose();
      }
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey, true);
    };
  }, [open, onClose]);

  if (!open || typeof document === 'undefined') return null;

  return createPortal(
    <div
      ref={ref}
      role="menu"
      style={{ left: pos.left, top: pos.top, position: 'fixed' }}
      className="z-[100] min-w-[180px] rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-lg animate-fade-in"
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((it, idx) =>
        it.kind === 'divider' ? (
          <div key={`d-${idx}`} className="my-1 h-px bg-border" role="separator" />
        ) : (
          <button
            key={`i-${idx}`}
            role="menuitem"
            type="button"
            disabled={it.disabled}
            onClick={() => {
              if (it.disabled) return;
              it.onClick();
              onClose();
            }}
            className={cn(
              'flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs transition-colors',
              it.disabled
                ? 'cursor-not-allowed text-muted-foreground/50'
                : it.danger
                  ? 'text-red-400 hover:bg-red-500/10'
                  : 'text-foreground hover:bg-accent',
            )}
          >
            {it.icon && (
              <span className="flex h-3.5 w-3.5 shrink-0 items-center justify-center">
                {it.icon}
              </span>
            )}
            <span className="flex-1 truncate">{it.label}</span>
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}
