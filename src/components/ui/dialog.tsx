import { useEffect, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';
import { cn } from '@/lib/utils';
import { Button } from './button';

interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  className?: string;
  /**
   * When true, clicking the backdrop will NOT close the dialog. Use for blocking
   * confirms where the user must explicitly choose an action.
   */
  disableBackdropClose?: boolean;
}

/**
 * Minimal shadcn-style dialog primitive — no Radix, no portal library.
 *
 * - Renders into `document.body` via a portal so it escapes any overflow
 *   parent in the tree.
 * - Closes on Escape keypress and (by default) backdrop click.
 * - Locks body scroll while open.
 * - Keeps the existing focus context; callers can `autoFocus` the first field.
 */
export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  className,
  disableBackdropClose,
}: DialogProps) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onOpenChange(false);
      }
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  }, [open, onOpenChange]);

  useEffect(() => {
    if (!open) return;
    const prevOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      document.body.style.overflow = prevOverflow;
    };
  }, [open]);

  if (!open || typeof document === 'undefined') return null;

  return createPortal(
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center p-4 animate-fade-in"
      onClick={(e) => {
        // Backdrop click closes — but the inner backdrop div captures
        // the click, so target !== currentTarget. Use closest('[role="dialog"]')
        // to find the panel and check whether the click was inside it.
        if (disableBackdropClose) return;
        const dialog = (e.currentTarget as HTMLElement).querySelector('[role="dialog"]');
        if (dialog && !dialog.contains(e.target as Node)) onOpenChange(false);
      }}
    >
      <div className="absolute inset-0 bg-black/50 backdrop-blur-sm" aria-hidden="true" />
      <div
        role="dialog"
        aria-modal="true"
        aria-label={typeof title === 'string' ? title : undefined}
        className={cn(
          'relative z-10 w-full max-w-md rounded-lg border border-border bg-card text-card-foreground shadow-2xl',
          className,
        )}
      >
        <div className="flex items-start justify-between gap-3 border-b border-border px-4 py-3">
          <div className="min-w-0 flex-1">
            <h2 className="text-sm font-semibold leading-5">{title}</h2>
            {description && (
              <p className="mt-0.5 text-xs text-muted-foreground">{description}</p>
            )}
          </div>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6 shrink-0 -mr-1 -mt-1"
            onClick={() => onOpenChange(false)}
            aria-label="关闭"
          >
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
        {children && <div className="px-4 py-3">{children}</div>}
        {footer && (
          <div className="flex items-center justify-end gap-2 border-t border-border px-4 py-3">
            {footer}
          </div>
        )}
      </div>
    </div>,
    document.body,
  );
}
