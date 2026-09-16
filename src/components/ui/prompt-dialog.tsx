import { useEffect, useState, type ReactNode } from 'react';
import { Dialog } from './dialog';
import { Input } from './input';
import { Button } from './button';

interface PromptDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  /** Input placeholder. */
  placeholder?: string;
  /** Initial value when the dialog opens. */
  defaultValue?: string;
  /**
   * Optional synchronous validator. Return null to accept, or a string
   * to show as an inline error. The Confirm button stays disabled
   * while there's a validation error.
   */
  validate?: (value: string) => string | null;
  confirmText?: string;
  cancelText?: string;
  onConfirm: (value: string) => void;
}

/**
 * A styled replacement for the browser's `window.prompt`:
 *   - Custom title / description (browser prompt shows the page host
 *     "localhost:5173 显示" instead, which is what users complained
 *     about).
 *   - Custom position (rendered through a portal — centered, can be
 *     repositioned later).
 *   - Keyboard: Enter confirms, Escape closes (handled by Dialog).
 *
 * Returns the trimmed value via `onConfirm`. If the user cancels or
 * the input is empty / invalid, `onConfirm` is not called.
 */
export function PromptDialog({
  open,
  onOpenChange,
  title,
  description,
  placeholder,
  defaultValue,
  validate,
  confirmText = '确定',
  cancelText = '取消',
  onConfirm,
}: PromptDialogProps) {
  const [value, setValue] = useState(defaultValue ?? '');
  const [error, setError] = useState<string | null>(null);

  // Reset every time it opens so a cancelled-then-reopened dialog
  // doesn't carry stale text.
  useEffect(() => {
    if (open) {
      setValue(defaultValue ?? '');
      setError(null);
    }
  }, [open, defaultValue]);

  const trimmed = value.trim();
  const liveError = validate ? validate(trimmed) : null;
  const canConfirm = trimmed.length > 0 && !liveError;

  const commit = () => {
    if (!canConfirm) return;
    onConfirm(trimmed);
    onOpenChange(false);
  };

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
          <Button onClick={commit} disabled={!canConfirm}>
            {confirmText}
          </Button>
        </>
      }
    >
      <Input
        autoFocus
        value={value}
        onChange={(e) => {
          setValue(e.target.value);
          if (error) setError(null);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            commit();
          }
        }}
        placeholder={placeholder}
        spellCheck={false}
        aria-invalid={!!liveError || undefined}
      />
      {(liveError || error) && (
        <p className="mt-1.5 text-[11px] text-destructive">
          {liveError ?? error}
        </p>
      )}
    </Dialog>
  );
}