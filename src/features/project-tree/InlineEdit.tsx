import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { cn } from '@/lib/utils';

interface InlineEditProps {
  /** The current persisted value — also the value to revert to on cancel. */
  value: string;
  /** Called with the trimmed new name on Enter or blur (when non-empty). */
  onSave: (next: string) => void;
  /** Called on Esc, or on blur when the value is empty/unchanged. */
  onCancel: () => void;
  className?: string;
  inputClassName?: string;
  placeholder?: string;
  maxLength?: number;
}

/**
 * Tiny in-place editor for tree-row labels. Renders an `<input>` that
 * auto-focuses and selects the existing text on mount.
 *
 * Keyboard:
 *   - Enter  → commit (save if non-empty + changed, else cancel)
 *   - Esc    → cancel (revert)
 *   - Blur   → commit
 *
 * Event propagation is stopped so clicks / double-clicks don't bubble up
 * to the parent row (which would otherwise toggle expansion).
 */
export function InlineEdit({
  value,
  onSave,
  onCancel,
  className,
  inputClassName,
  placeholder,
  maxLength,
}: InlineEditProps) {
  const [draft, setDraft] = useState(value);
  const cancelledRef = useRef(false);
  const committedRef = useRef(false);
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.focus();
    el.select();
  }, []);

  const commit = () => {
    // Guard against double-fire: Enter fires the keydown handler *and* the
    // input blurs as React unmounts. Mark committed on the first call.
    if (committedRef.current) return;
    committedRef.current = true;
    if (cancelledRef.current) return;
    const next = draft.trim();
    if (next.length > 0 && next !== value) {
      onSave(next);
    } else {
      onCancel();
    }
  };

  const handleKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      e.stopPropagation();
      commit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      cancelledRef.current = true;
      onCancel();
    }
  };

  return (
    <span
      className={cn('flex w-full min-w-0', className)}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
    >
      <input
        ref={ref}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={handleKey}
        onBlur={commit}
        maxLength={maxLength}
        placeholder={placeholder}
        className={cn(
          'h-6 w-full min-w-0 rounded-sm border border-ring bg-background px-1.5 text-xs text-foreground focus:outline-none',
          inputClassName,
        )}
      />
    </span>
  );
}
