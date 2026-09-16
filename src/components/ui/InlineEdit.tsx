import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from 'react';
import { cn } from '@/lib/utils';

export interface InlineEditProps {
  /** The current persisted value — also the value to revert to on cancel. */
  value: string;
  /**
   * Called with the (optionally trimmed) new value on Enter / blur when
   * the new value is non-empty AND differs from `value`. The component
   * intentionally skips no-op commits so consumers don't have to diff.
   */
  onSave: (next: string) => void;
  /**
   * Render-prop for the display element. The component wraps the
   * returned node in a `<span>` that listens for `onDoubleClick` and
   * forwards an `open` callback so the caller can wire the trigger
   * however they like (e.g. a button, an icon, the value text itself).
   */
  display: (trigger: { open: () => void }) => ReactNode;
  placeholder?: string;
  className?: string;
  /**
   * Trim whitespace from the draft before commit/cancel. Defaults to
   * `true` to match the tree-row rename UX; set `false` for fields
   * that legitimately allow leading/trailing whitespace (rare).
   */
  trim?: boolean;
  ariaLabel?: string;
  testId?: string;
  /**
   * Notified whenever the editor enters or leaves edit mode. Useful for
   * parents that want to gate other UX (e.g. marking a tab as having
   * unsaved changes during an in-flight rename).
   */
  onEditingChange?: (editing: boolean) => void;
  /**
   * When `true`, an empty draft is committed as `onSave('')` instead of
   * being treated as a cancel. Defaults to `false` (blank = cancel) to
   * match the tree-row rename UX. Set `true` for fields that legitimately
   * allow clearing (e.g. an optional description).
   */
  commitEmpty?: boolean;
}

/**
 * Generic inline editor. Renders the `display` node by default; on
 * double-click (or via `display`'s `open()` callback) swaps in a
 * single-line `<input>` that auto-focuses and selects the existing
 * text. Keyboard:
 *   - Enter  → commit (save if non-empty + changed, else cancel —
 *              unless `commitEmpty`, in which case an empty draft is
 *              committed as `onSave('')`)
 *   - Esc    → cancel (revert draft to `value`)
 *   - Blur   → commit
 *
 * When in edit mode, click and Enter events on the `<input>` are stopped
 * from propagating. This is the right default for an inline editor that
 * is typically nested inside a clickable row (the row shouldn't collapse
 * while the user is editing) and alongside siblings that may bind to
 * Enter (e.g. a TagInput). Callers that actually need bubbling can wrap
 * the editor in their own click target.
 */
export function InlineEdit({
  value,
  onSave,
  display,
  placeholder,
  className,
  trim = true,
  ariaLabel,
  testId,
  onEditingChange,
  commitEmpty = false,
}: InlineEditProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const inputRef = useRef<HTMLInputElement>(null);
  // Guard against double-fire when Enter triggers both a keydown and a
  // synthetic blur as React unmounts the editing branch.
  const committedRef = useRef(false);
  const cancelledRef = useRef(false);

  // Re-sync the local draft when the underlying value changes outside
  // an active edit session (e.g. the tree updates from another panel).
  useEffect(() => {
    if (!editing) setDraft(value);
  }, [value, editing]);

  // Auto-focus + select on entering edit mode.
  useEffect(() => {
    if (!editing) return;
    const el = inputRef.current;
    if (!el) return;
    el.focus();
    el.select();
  }, [editing]);

  const open = useCallback(() => {
    cancelledRef.current = false;
    committedRef.current = false;
    setDraft(value);
    setEditing(true);
  }, [value]);

  const cancel = useCallback(() => {
    cancelledRef.current = true;
    setDraft(value);
    setEditing(false);
  }, [value]);

  const commit = useCallback(() => {
    if (committedRef.current) return;
    if (cancelledRef.current) {
      setEditing(false);
      return;
    }
    committedRef.current = true;
    const next = trim ? draft.trim() : draft;
    const shouldSave = commitEmpty ? next !== value : next.length > 0 && next !== value;
    if (shouldSave) onSave(next);
    setEditing(false);
  }, [draft, value, onSave, trim, commitEmpty]);

  // Bridge editing state for parents that want to gate other UX (tab dirty
  // markers, modal locks, etc.).
  useEffect(() => {
    onEditingChange?.(editing);
  }, [editing, onEditingChange]);

  const handleKey = (e: KeyboardEvent<HTMLInputElement>) => {
    // Stop Enter from bubbling — siblings (e.g. TagInput) may bind Enter
    // and would otherwise treat a committed rename as their own action.
    if (e.key === 'Enter') {
      e.preventDefault();
      e.stopPropagation();
      commit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      cancel();
    }
  };

  const stopClick: React.MouseEventHandler<HTMLInputElement> = (e) => {
    // The input itself swallows clicks so the surrounding row's onClick
    // (collapse/expand) doesn't fire mid-edit.
    e.stopPropagation();
  };

  if (!editing) {
    return (
      <span
        onDoubleClick={open}
        // `inline-flex` keeps the wrapper on the same line as the
        // surrounding text and lets `display` size itself naturally.
        className={cn('inline-flex', className)}
      >
        {display({ open })}
      </span>
    );
  }

  return (
    <input
      ref={inputRef}
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onClick={stopClick}
      onKeyDown={handleKey}
      onBlur={commit}
      placeholder={placeholder}
      aria-label={ariaLabel}
      data-testid={testId}
      className={cn(
        'h-6 w-full min-w-0 rounded-sm border border-ring bg-background px-1.5 text-xs text-foreground focus:outline-none',
        className,
      )}
    />
  );
}