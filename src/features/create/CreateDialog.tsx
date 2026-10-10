/**
 * The create dialogs' shared chrome — the `Dialog` frame, the submit
 * guard, and the two fields every kind has.
 *
 * WHY THIS IS NOT A "NewItemModal"
 * -------------------------------
 * There used to be one modal with a type-chooser first stage, which meant
 * every entry point — the welcome screen, the tree, the top bar, the
 * command palette — shared a component that had to know about all four
 * kinds and could be opened in a state none of them asked for. The kinds
 * are now separate components that live next to their own entry points
 * (see `CreateProjectDialog` / `CreateModuleDialog` /
 * `CreateCollectionDialog`), and this file holds only what they have in
 * common. An endpoint has no dialog at all: `createEndpointAt` writes the
 * row directly.
 */

import { useEffect, type FormEvent, type ReactNode, type RefObject } from 'react';
import { Dialog } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import { PROJECT_COLORS } from '@/lib/palette';

interface CreateDialogProps {
  open: boolean;
  onClose: () => void;
  /** Dialog title, e.g. 「新建模块」. */
  title: string;
  /** Optional one-line explanation under the title. */
  description?: string;
  /** False while the required fields are still empty. */
  canSubmit: boolean;
  onSubmit: () => void;
  /** Focused on open — the name is the only field you always have to fill. */
  nameInputRef: RefObject<HTMLInputElement>;
  children: ReactNode;
  /** Widen the frame for the project form (it has the widest field set). */
  wide?: boolean;
}

/**
 * Frame shared by the three create dialogs: submit-on-Enter, focus the
 * name field on open, and a footer whose primary button reflects the
 * guard rather than being clicked into a no-op.
 */
export function CreateDialog({
  open,
  onClose,
  title,
  description,
  canSubmit,
  onSubmit,
  nameInputRef,
  children,
  wide,
}: CreateDialogProps) {
  // Deferred one tick: the input only exists once the dialog has
  // rendered, and `Dialog` mounts its portal on the open transition.
  useEffect(() => {
    if (!open) return;
    const t = window.setTimeout(() => nameInputRef.current?.focus(), 0);
    return () => window.clearTimeout(t);
  }, [open, nameInputRef]);

  if (!open) return null;

  const handleSubmit = (e?: FormEvent) => {
    e?.preventDefault();
    if (!canSubmit) return;
    onSubmit();
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
      title={title}
      description={description}
      className={wide ? 'max-w-lg' : undefined}
      footer={
        <>
          <Button variant="ghost" size="sm" onClick={onClose}>
            取消
          </Button>
          <Button size="sm" onClick={() => handleSubmit()} disabled={!canSubmit}>
            创建
          </Button>
        </>
      }
    >
      <form onSubmit={handleSubmit} className="space-y-3">
        {children}
      </form>
    </Dialog>
  );
}

export function Field({
  label,
  required,
  children,
}: {
  label: string;
  required?: boolean;
  children: ReactNode;
}) {
  return (
    <label className="block space-y-1">
      <span className="text-xs font-medium text-muted-foreground">
        {label}
        {required && <span className="ml-0.5 text-destructive">*</span>}
      </span>
      {children}
    </label>
  );
}

export function NameField({
  value,
  onChange,
  inputRef,
  placeholder = '输入名称',
}: {
  value: string;
  onChange: (next: string) => void;
  inputRef: RefObject<HTMLInputElement>;
  placeholder?: string;
}) {
  return (
    <Field label="名称" required>
      <Input
        ref={inputRef}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        autoFocus
        data-testid="new-item-name"
      />
    </Field>
  );
}

export function DescriptionField({
  value,
  onChange,
  placeholder = '一句话说明（可选）',
}: {
  value: string;
  onChange: (next: string) => void;
  placeholder?: string;
}) {
  return (
    <Field label="描述">
      <Input value={value} onChange={(e) => onChange(e.target.value)} placeholder={placeholder} />
    </Field>
  );
}

/**
 * Project colour swatches. Only the project dialog has one: a colour is
 * what tells two projects apart in the welcome list and the tree header,
 * and neither a module nor a collection has anywhere to show it.
 */
export function ColorPicker({ value, onChange }: { value: string; onChange: (c: string) => void }) {
  return (
    <div className="flex items-center gap-1.5">
      {PROJECT_COLORS.map((c) => (
        <button
          key={c}
          type="button"
          onClick={() => onChange(c)}
          aria-label={`选择颜色 ${c}`}
          className={cn(
            'h-5 w-5 rounded-full transition-all',
            value === c
              ? 'ring-2 ring-ring ring-offset-2 ring-offset-card'
              : 'opacity-70 hover:opacity-100',
          )}
          style={{ backgroundColor: c }}
        />
      ))}
    </div>
  );
}
