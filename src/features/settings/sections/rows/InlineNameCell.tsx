import { useEffect, useRef, useState } from 'react';
import { Pencil } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';

/**
 * A compact click-to-edit text cell. Click the value to swap in an
 * input; blur or Enter commits, Escape reverts. Optional `validate`
 * rejects commits with an inline error message.
 *
 * Kept local to the environment section (rather than reusing
 * `@/components/ui/InlineEdit`) because it opens on a single click —
 * the row-dense table layout here has no room for a double-click
 * affordance — and because it supports `validate` + inline error
 * rendering, which `InlineEdit` has no equivalent for.
 *
 * `autoEdit` opens the input without a click. The env panel uses it to
 * drop a freshly created env straight into its name so the user never
 * has to hunt for the rename affordance; every other call site (service
 * names, variable keys) keeps driving the open by click.
 */
export function InlineNameCell({
  value,
  placeholder,
  validate,
  onCommit,
  autoEdit,
  onAutoEditEnd,
  className,
}: {
  value: string;
  placeholder: string;
  validate?: (next: string) => string | null;
  onCommit: (next: string) => void;
  /**
   * Open in rename mode on mount / on the flag's false → true edge. The
   * owner clears it via `onAutoEditEnd` once consumed.
   */
  autoEdit?: boolean;
  /** Fired the moment the `autoEdit` open is consumed, so the owner can drop the flag. */
  onAutoEditEnd?: () => void;
  className?: string;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  // `autoEdit` is a flag, not an event: it can stay `true` across
  // re-renders (and the cell can be remounted later, e.g. switching envs
  // re-keys the body). A ref latch turns it into a false → true edge so
  // we open exactly once per raise, never on every keystroke re-render.
  const autoOpened = useRef(false);
  // Only the auto-open preselects the text. A click-to-edit on an
  // existing row keeps its previous caret-at-end behaviour, so renaming
  // a service name is not silently a select-all-replace.
  const preselectOnOpen = useRef(false);

  useEffect(() => {
    if (!autoEdit) {
      autoOpened.current = false;
      return;
    }
    if (autoOpened.current) return;
    autoOpened.current = true;
    preselectOnOpen.current = true;
    setDraft(value);
    setError(null);
    setEditing(true);
    onAutoEditEnd?.();
  }, [autoEdit, value, onAutoEditEnd]);

  // Runs one render after `editing` flips, which is the first point the
  // input actually exists — the effect above cannot reach the ref.
  useEffect(() => {
    if (!editing || !preselectOnOpen.current) return;
    preselectOnOpen.current = false;
    inputRef.current?.select();
  }, [editing]);

  const begin = () => {
    setDraft(value);
    setError(null);
    setEditing(true);
  };
  const cancel = () => {
    setEditing(false);
    setError(null);
  };
  const commit = () => {
    const trimmed = draft.trim();
    if (validate) {
      const msg = validate(trimmed);
      if (msg) {
        setError(msg);
        return;
      }
    }
    if (trimmed !== value) onCommit(trimmed);
    setEditing(false);
    setError(null);
  };

  if (!editing) {
    return (
      <button
        type="button"
        onClick={begin}
        className={cn(
          'group/cell flex w-full items-center gap-1 rounded px-1.5 py-0.5 text-left transition-colors hover:bg-accent/40',
          !value && 'text-muted-foreground/60 italic',
          className,
        )}
      >
        <span className="min-w-0 truncate">{value || placeholder}</span>
        <Pencil className="h-3 w-3 shrink-0 opacity-0 transition-opacity group-hover/cell:opacity-100" />
      </button>
    );
  }

  return (
    <div className="flex flex-col gap-1">
      <Input
        ref={inputRef}
        autoFocus
        value={draft}
        onChange={(e) => {
          setDraft(e.target.value);
          if (error) setError(null);
        }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            commit();
          } else if (e.key === 'Escape') {
            e.preventDefault();
            cancel();
          }
        }}
        className="h-7 font-mono text-xs"
      />
      {error && <div className="px-1 text-[10px] text-destructive">{error}</div>}
    </div>
  );
}
