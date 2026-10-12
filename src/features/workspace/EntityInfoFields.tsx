/**
 * AutoSaveField — the form primitives behind every entity's 基础信息 layer.
 *
 * WHY AUTOSAVE AND NOT A SAVE BUTTON
 * ---------------------------------
 * These fields write through `updateProject` / `updateNode`, which persist
 * immediately, and every other configuration surface in the app already
 * behaves that way (the settings panel says so outright: 设置改动会立即保存).
 * A "编辑 / 保存" pair here would imply a staging area that does not exist,
 * and would leave the workspace as the one place where closing a tab can
 * lose a word. So: commit on blur, commit on Enter, revert on Escape, and
 * write nothing when the value did not change.
 *
 * The draft is local, and the one rule that makes that safe is the
 * re-sync effect: a value arriving from *outside* (a rename in the tree,
 * the store catching up) must replace the draft, but only while the field
 * is not being typed in — otherwise a background write would eat the
 * characters under the caret.
 */

import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';

interface AutoSaveInputProps {
  value: string;
  /** Called with the trimmed value when it changed. */
  onSave: (next: string) => void;
  placeholder?: string;
  ariaLabel: string;
  className?: string;
}

export function AutoSaveInput({
  value,
  onSave,
  placeholder,
  ariaLabel,
  className,
}: AutoSaveInputProps) {
  const [draft, setDraft] = useState(value);
  const focused = useRef(false);

  useEffect(() => {
    if (!focused.current) setDraft(value);
  }, [value]);

  const commit = () => {
    const next = draft.trim();
    if (next.length > 0 && next !== value) onSave(next);
    else setDraft(value);
  };

  return (
    <Input
      value={draft}
      placeholder={placeholder}
      aria-label={ariaLabel}
      spellCheck={false}
      onFocus={() => {
        focused.current = true;
      }}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        focused.current = false;
        commit();
      }}
      onKeyDown={(e: KeyboardEvent<HTMLInputElement>) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          e.currentTarget.blur();
        } else if (e.key === 'Escape') {
          // Revert, then leave: a cancelled edit should not also save.
          e.preventDefault();
          setDraft(value);
          e.currentTarget.blur();
        }
      }}
      className={cn('h-8 text-xs', className)}
    />
  );
}

interface AutoSaveTextareaProps {
  /** `undefined` and `''` both read as "no description". */
  value: string | undefined;
  /** Called with the trimmed value, including `''` to clear it. */
  onSave: (next: string) => void;
  placeholder?: string;
  ariaLabel: string;
  className?: string;
}

export function AutoSaveTextarea({
  value,
  onSave,
  placeholder,
  ariaLabel,
  className,
}: AutoSaveTextareaProps) {
  const [draft, setDraft] = useState(value ?? '');
  const focused = useRef(false);

  useEffect(() => {
    if (!focused.current) setDraft(value ?? '');
  }, [value]);

  const commit = () => {
    const next = draft.trim();
    if (next !== (value ?? '')) onSave(next);
    else setDraft(value ?? '');
  };

  return (
    <textarea
      value={draft}
      placeholder={placeholder}
      aria-label={ariaLabel}
      spellCheck={false}
      rows={3}
      onFocus={() => {
        focused.current = true;
      }}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        focused.current = false;
        commit();
      }}
      onKeyDown={(e: KeyboardEvent<HTMLTextAreaElement>) => {
        // Enter inserts a newline here — a description is prose. Ctrl/⌘+Enter
        // is the commit, and Escape abandons the edit.
        if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
          e.preventDefault();
          e.currentTarget.blur();
        } else if (e.key === 'Escape') {
          e.preventDefault();
          setDraft(value ?? '');
          e.currentTarget.blur();
        }
      }}
      className={cn(
        'w-full resize-y rounded-md border border-input bg-background px-2 py-1.5 text-xs text-foreground',
        'placeholder:text-muted-foreground/70 focus:border-ring focus:outline-none',
        className,
      )}
    />
  );
}

/**
 * One labelled row inside the 基础信息 card.
 *
 * Vertical (label above control), because the controls here are full-width
 * text fields and the settings panel's `SettingRow` puts its control on the
 * right — a layout that works for a switch or a small button and collapses
 * for a textarea.
 */
export function InfoField({
  label,
  hint,
  className,
  children,
}: {
  label: string;
  hint?: string;
  /** Stacking/separation for the row — e.g. `border-t` between two of them. */
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <div className={cn('space-y-1.5 px-4 py-3', className)}>
      <div className="text-xs font-medium text-foreground">{label}</div>
      {children}
      {hint && <p className="text-[11px] text-muted-foreground">{hint}</p>}
    </div>
  );
}
