import { useState } from 'react';
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
 */
export function InlineNameCell({
  value,
  placeholder,
  validate,
  onCommit,
  className,
}: {
  value: string;
  placeholder: string;
  validate?: (next: string) => string | null;
  onCommit: (next: string) => void;
  className?: string;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const [error, setError] = useState<string | null>(null);

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
