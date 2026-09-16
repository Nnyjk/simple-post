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
   * Renders the confirm button in destructive style (red). Use for
   * deletes and other irreversible actions.
   */
  destructive?: boolean;
  onConfirm: () => void;
}

/**
 * A styled replacement for the browser's `window.confirm`. Same
 * motivation as PromptDialog — fixed positioning, custom title (the
 * browser labels confirm dialogs "localhost:5173 显示"), and a
 * styled destructive variant.
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