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
 * The 基础信息 list — hairlines between rows, and no card around them.
 *
 * This block used to be a `rounded-lg border bg-card/40` shell with the rows
 * inside it, which put a card inside a section that is already a titled
 * block, holding a card-ish `Input` and `textarea` of its own. Three borders
 * deep for two text fields. The section heading is the boundary that is
 * doing the work here, so the rows only need hairlines between them, and
 * they run the full width of the section — the same list shape
 * `SettingRow` produces in the settings panel, so the workspace's own rows
 * and the settings rows it sits above read as one kind of thing.
 */
export function InfoBlock({ children }: { children: React.ReactNode }) {
  return <div className="divide-y divide-border/60 border-y border-border/60">{children}</div>;
}

/**
 * One labelled row inside `InfoBlock`.
 *
 * Label and hint on the left, control on the right — the `SettingRow`
 * layout, deliberately, so these two fields do not introduce a third
 * settings shape next to the ones the settings panel already renders. It
 * also puts the hint under its own label instead of trailing the control,
 * which is where a hint belongs: the old stacking had 描述's keyboard
 * legend sitting between the textarea and the divider below it.
 */
export function InfoField({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-start gap-6 py-3">
      <div className="w-40 shrink-0">
        <div className="text-xs font-medium text-foreground">{label}</div>
        {hint && <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">{hint}</p>}
      </div>
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}
