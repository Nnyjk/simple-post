import { useId, type ReactNode } from 'react';
import { X } from 'lucide-react';
import { cn } from '@/lib/utils';
import { ModalSurface } from './modal-surface';
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
 * 模态对话框 —— 标题、说明、正文、页脚四段。
 *
 * 模态语义（portal、Esc、点遮罩、滚动锁、焦点陷阱、背景 inert、入场动画）都在
 * `ModalSurface` 里，本组件只负责面板长什么样。
 *
 * 面板的可访问名走标题元素的 id（`aria-labelledby`），不靠 `aria-label` 抄一份
 * 标题文字：抄的那份与看到的标题会各自漂移，标题改成非字符串时更是直接消失。
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
  const titleId = useId();
  const descriptionId = useId();

  return (
    <ModalSurface
      open={open}
      onOpenChange={onOpenChange}
      label={{ ariaLabelledBy: titleId }}
      ariaDescribedBy={description ? descriptionId : undefined}
      className={cn('bg-card text-card-foreground', className)}
      disableBackdropClose={disableBackdropClose}
    >
      <div className="flex items-start justify-between gap-3 border-b border-border px-4 py-3">
        <div className="min-w-0 flex-1">
          <h2 id={titleId} className="text-sm font-semibold leading-5">
            {title}
          </h2>
          {description && (
            <p id={descriptionId} className="mt-0.5 text-xs text-muted-foreground">
              {description}
            </p>
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
    </ModalSurface>
  );
}
