import type { ReactNode } from 'react';
import { Dialog } from './dialog';
import { Button } from './button';

interface ConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  confirmText?: string;
  cancelText?: string;
  /**
   * 把确认按钮渲染成破坏性样式（红）。用于删除与其他不可逆动作。
   */
  destructive?: boolean;
  onConfirm: () => void;
}

/**
 * 浏览器 `window.confirm` 的样式化替代。动机与 PromptDialog 相同——fixed 定位、
 * 自定义标题（浏览器会把确认框标成「localhost:5173 显示」）、以及一个样式化的
 * 破坏性变体。
 */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmText = '确定',
  cancelText = '取消',
  destructive = false,
  onConfirm,
}: ConfirmDialogProps) {
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      description={description}
      footer={
        <>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {cancelText}
          </Button>
          <Button
            variant={destructive ? 'destructive' : 'default'}
            onClick={() => {
              onConfirm();
              onOpenChange(false);
            }}
          >
            {confirmText}
          </Button>
        </>
      }
    />
  );
}
