import { useEffect, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';
import { cn } from '@/lib/utils';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';
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
   * 为 true 时点背景不关闭对话框。用于必须让用户显式选一个动作的阻塞式确认框。
   */
  disableBackdropClose?: boolean;
}

/**
 * 极简 shadcn 风格对话框原语——无 Radix，无 portal 库。
 *
 * - 经 portal 渲染进 `document.body`，好逃出树里任何 overflow 父级。
 * - Esc 键与（默认）点背景都关闭。
 * - 打开时锁住 body 滚动。
 * - 保留既有焦点上下文；调用方可对第一个字段 `autoFocus`。
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

  const backdropRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  // 入场动画——每次对话框挂载（open 翻成 true）都跑。没有出场动画：`open=false`
  // 会整体卸载 portal，这样就够。
  //
  // 减弱动效下对话框直接出现。它照样挂载、照样困住焦点、照样有背景，所以它的
  // 行为没有任何一点依赖这段过渡跑起来。
  useGSAP(
    () => {
      return motionSafe(() => {
        if (backdropRef.current) {
          gsap.fromTo(
            backdropRef.current,
            { opacity: 0 },
            { opacity: 1, duration: 0.18, ease: 'none' },
          );
        }
        if (panelRef.current) {
          gsap.fromTo(
            panelRef.current,
            { opacity: 0, scale: 0.96 },
            { opacity: 1, scale: 1, duration: 0.22, ease: 'power2.out' },
          );
        }
      });
    },
    { scope: panelRef, dependencies: [open] },
  );

  if (!open || typeof document === 'undefined') return null;

  return createPortal(
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center p-4"
      onClick={(e) => {
        // 点背景关闭——但内层背景 div 会吃掉点击，所以 target !== currentTarget。
        // 用 closest('[role="dialog"]') 找到面板，判断点击是否落在它里面。
        if (disableBackdropClose) return;
        const dialog = (e.currentTarget as HTMLElement).querySelector('[role="dialog"]');
        if (dialog && !dialog.contains(e.target as Node)) onOpenChange(false);
      }}
    >
      <div
        ref={backdropRef}
        className="absolute inset-0 bg-black/50 backdrop-blur-sm"
        aria-hidden="true"
      />
      <div
        ref={panelRef}
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
            {description && <p className="mt-0.5 text-xs text-muted-foreground">{description}</p>}
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
