/**
 * TagInput — controlled chip-based input for a `string[]` value.
 *
 * Behavior:
 *  - Type a token, press Enter or `,` to commit it as a tag.
 *  - Backspace on an empty draft input removes the last tag.
 *  - Click the `x` on a chip to remove that tag.
 *  - Duplicate tags are de-duplicated (case-insensitive).
 *
 * No new dependencies; styling re-uses the shadcn-style border / muted vars
 * already defined in `globals.css`.
 */

import { useState, useRef, type KeyboardEvent, type ClipboardEvent } from 'react';
import { X } from 'lucide-react';
import { cn } from '@/lib/utils';

export interface TagInputProps {
  value: string[];
  onChange: (next: string[]) => void;
  placeholder?: string;
  className?: string;
  disabled?: boolean;
  /** Max tags allowed; default unlimited. */
  maxTags?: number;
}

function normalize(tag: string): string {
  return tag.trim();
}

export function TagInput({
  value,
  onChange,
  placeholder = '输入后回车或逗号添加…',
  className,
  disabled,
  maxTags,
}: TagInputProps) {
  const [draft, setDraft] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  const commit = (raw: string) => {
    const tag = normalize(raw);
    if (!tag) return;
    const exists = value.some((t) => t.toLowerCase() === tag.toLowerCase());
    if (exists) {
      setDraft('');
      return;
    }
    if (maxTags !== undefined && value.length >= maxTags) {
      setDraft('');
      return;
    }
    onChange([...value, tag]);
    setDraft('');
  };

  const removeAt = (idx: number) => {
    onChange(value.filter((_, i) => i !== idx));
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (disabled) return;
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      commit(draft);
    } else if (e.key === 'Backspace' && draft === '' && value.length > 0) {
      e.preventDefault();
      removeAt(value.length - 1);
    } else if (e.key === 'Delete' && draft === '' && value.length > 0) {
      // Allow forward delete on empty draft too.
      e.preventDefault();
      removeAt(0);
    }
  };

  const onPaste = (e: ClipboardEvent<HTMLInputElement>) => {
    const text = e.clipboardData.getData('text');
    if (!text) return;
    if (/[,\n]/.test(text)) {
      e.preventDefault();
      const parts = text
        .split(/[,\n]/)
        .map((p) => normalize(p))
        .filter(Boolean);
      if (parts.length === 0) return;
      // de-dupe against existing
      const seen = new Set(value.map((t) => t.toLowerCase()));
      const merged = [...value];
      for (const p of parts) {
        const k = p.toLowerCase();
        if (seen.has(k)) continue;
        seen.add(k);
        if (maxTags !== undefined && merged.length >= maxTags) break;
        merged.push(p);
      }
      onChange(merged);
      setDraft('');
    }
  };

  return (
    <div
      onClick={() => inputRef.current?.focus()}
      className={cn(
        'flex min-h-8 w-full flex-wrap items-center gap-1 rounded-md border border-input bg-background px-1.5 py-1 text-sm',
        'focus-within:ring-2 focus-within:ring-ring focus-within:ring-offset-1 focus-within:ring-offset-background',
        disabled && 'cursor-not-allowed opacity-50',
        className,
      )}
    >
      {value.map((tag, idx) => (
        <span
          key={`${tag}-${idx}`}
          className="inline-flex items-center gap-1 rounded-md border border-border bg-muted/60 px-1.5 py-0.5 text-[11px] font-medium text-foreground"
        >
          {tag}
          {!disabled && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                removeAt(idx);
              }}
              className="inline-flex h-3.5 w-3.5 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
              aria-label={`删除标签 ${tag}`}
            >
              <X className="h-2.5 w-2.5" />
            </button>
          )}
        </span>
      ))}
      <input
        ref={inputRef}
        value={draft}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={onKeyDown}
        onPaste={onPaste}
        onBlur={() => {
          // Commit any pending draft on blur so users don't lose typed text.
          if (draft.trim()) commit(draft);
        }}
        placeholder={value.length === 0 ? placeholder : ''}
        className="min-w-[8ch] flex-1 bg-transparent text-sm text-foreground placeholder:text-muted-foreground/60 focus:outline-none disabled:cursor-not-allowed"
      />
    </div>
  );
}
