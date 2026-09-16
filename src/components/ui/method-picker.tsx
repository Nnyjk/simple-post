/**
 * MethodPicker — custom listbox for selecting an HTTP method.
 *
 * Replaces the native <select> previously used in HttpTester's URL bar.
 * The native <select> cannot style its <option> children independently of
 * the trigger, so picking a colored method (e.g. POST → blue) would bleed
 * the color into every other option. This component renders each option as
 * a real <button>, so each one can be colored by its own `methodColorVar`.
 *
 * Interaction:
 *  - Click trigger → toggle open / close
 *  - Click outside (mousedown) → close
 *  - Esc → close
 *  - ↑ / ↓ → move active index (does not commit)
 *  - Enter → commit active option and close
 *  - Tab → close (default tab navigation is preserved)
 *  - Click an option → commit and close
 */

import { useEffect, useRef, useState } from 'react';
import { ChevronDown } from 'lucide-react';
import { cn, methodColorVar } from '@/lib/utils';
import { HTTP_METHODS, type HttpMethod } from '@/types/domain';

export interface MethodPickerProps {
  value: HttpMethod;
  onChange: (m: HttpMethod) => void;
  className?: string;
}

export function MethodPicker({ value, onChange, className }: MethodPickerProps) {
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState<number>(() => {
    const i = HTTP_METHODS.indexOf(value);
    return i < 0 ? 0 : i;
  });

  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const optionRefs = useRef<(HTMLButtonElement | null)[]>([]);

  // Each time the picker opens, re-anchor the keyboard cursor to the
  // currently selected method. Mid-session ↑/↓ moves are not disturbed by
  // external value changes (those should be rare while the popover is up).
  useEffect(() => {
    if (!open) return;
    const i = HTTP_METHODS.indexOf(value);
    if (i >= 0 && i !== activeIndex) setActiveIndex(i);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, value]);

  // Focus follows activeIndex while open. Closing unmounts the list, so we
  // only need to drive focus while `open` is true.
  useEffect(() => {
    if (!open) return;
    const el = optionRefs.current[activeIndex];
    el?.focus();
  }, [open, activeIndex]);

  // Click outside closes.
  useEffect(() => {
    if (!open) return;
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (rootRef.current?.contains(target)) return;
      setOpen(false);
    };
    document.addEventListener('mousedown', onMouseDown);
    return () => document.removeEventListener('mousedown', onMouseDown);
  }, [open]);

  // Keyboard handling while the popover is open. We attach to `document` so
  // navigation works regardless of whether focus is on the trigger or on
  // an option (the option gets focus on open via the effect above).
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        setOpen(false);
        return;
      }
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setActiveIndex((i) => (i + 1) % HTTP_METHODS.length);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setActiveIndex((i) => (i - 1 + HTTP_METHODS.length) % HTTP_METHODS.length);
        return;
      }
      if (e.key === 'Enter') {
        e.preventDefault();
        const m = HTTP_METHODS[activeIndex];
        if (m) {
          onChange(m);
          setOpen(false);
        }
        return;
      }
      if (e.key === 'Tab') {
        // Per spec: do NOT preventDefault — let normal tab navigation
        // proceed. Just close the popover.
        setOpen(false);
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [open, activeIndex, onChange]);

  const handleSelect = (m: HttpMethod) => {
    onChange(m);
    setOpen(false);
  };

  return (
    <div ref={rootRef} className={cn('relative inline-block', className)}>
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={cn(
          'flex h-9 w-28 items-center justify-between rounded-md border border-input bg-background px-2 font-mono text-xs font-semibold uppercase',
          'transition-colors',
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background',
        )}
      >
        <span style={{ color: methodColorVar(value) }}>{value}</span>
        <ChevronDown
          className={cn(
            'h-3.5 w-3.5 text-muted-foreground transition-transform',
            open && 'rotate-180',
          )}
        />
      </button>
      {open && (
        <ul
          role="listbox"
          aria-label="HTTP method"
          className="absolute top-full left-0 z-50 mt-1 w-28 overflow-hidden rounded-md border border-border bg-popover py-1 shadow-lg"
        >
          {HTTP_METHODS.map((m, idx) => {
            const isSelected = m === value;
            const isActive = idx === activeIndex;
            return (
              <li key={m}>
                <button
                  ref={(el) => {
                    optionRefs.current[idx] = el;
                  }}
                  type="button"
                  role="option"
                  aria-selected={isSelected}
                  data-idx={idx}
                  onClick={() => handleSelect(m)}
                  onMouseEnter={() => setActiveIndex(idx)}
                  className={cn(
                    'flex w-full items-center justify-center px-2 py-1 font-mono text-xs font-semibold uppercase outline-none',
                    // hover/focus: tinted background, text reverts to
                    // foreground (the inline `color` style on hover would
                    // otherwise win — `!` important is required to surface
                    // the foreground color through it).
                    'hover:bg-accent hover:!text-foreground',
                    'focus:bg-accent focus:!text-foreground',
                    // selected / keyboard-active: subtle accent background
                    (isActive || isSelected) && 'bg-accent',
                  )}
                  style={{ color: methodColorVar(m) }}
                >
                  {m}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
