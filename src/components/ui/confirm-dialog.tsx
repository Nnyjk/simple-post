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
 * 确认框 —— `Dialog` 之上的薄封装，用于删除与其他不可逆动作。
 *
 * 初始焦点落在「取消」上，不是确认键。确认键多数是破坏性的，回车即删不是想要的默认；
 * 想让用户直接确认的场景不适用这个组件。
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
          <Button variant="ghost" autoFocus onClick={() => onOpenChange(false)}>
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
