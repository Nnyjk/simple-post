/**
 * EnvVarPicker — small popover that lists the active environment's variables
 * for quick insertion into a URL input as `{{key}}`.
 *
 * This is a controlled (presentational) component. The parent owns:
 *  - the `entries` to show (typically filtered by the partially-typed var name)
 *  - the `selected` row index and how to mutate it (`onSelectedChange`)
 *  - the open / close state
 *
 * Positioning is anchored to `anchorRef`. The popover is placed at
 * `top: input.bottom + 4px`, `left: input.left`, width >= 240px.
 */

import { useEffect, useRef } from 'react';
import { ChevronRight } from 'lucide-react';
import { cn } from '@/lib/utils';

export interface VarEntry {
  key: string;
  value: string;
}

interface EnvVarPickerProps {
  open: boolean;
  onClose: () => void;
  entries: VarEntry[];
  selected: number;
  onSelectedChange: (next: number) => void;
  onSelect: (key: string) => void;
  /** Anchor element (the URL input) used to position the popover. */
  anchorRef: React.RefObject<HTMLElement | null>;
  /** Optional caption shown in the header (e.g. active env name). */
  envName?: string;
}

function truncate(s: string | undefined, n: number): string {
  const str = s ?? '';
  return str.length <= n ? str : `${str.slice(0, n - 1)}…`;
}

export function EnvVarPicker({
  open,
  onClose,
  entries,
  selected,
  onSelectedChange,
  onSelect,
  anchorRef,
  envName,
}: EnvVarPickerProps) {
  const popRef = useRef<HTMLDivElement>(null);

  // Reposition whenever the popover opens or the window scrolls / resizes.
  useEffect(() => {
    if (!open) return;
    const setPos = () => {
      const el = popRef.current;
      const anchor = anchorRef.current;
      if (!el || !anchor) return;
      const rect = anchor.getBoundingClientRect();
      el.style.left = `${rect.left}px`;
      el.style.top = `${rect.bottom + 4}px`;
      el.style.minWidth = `${Math.max(240, rect.width)}px`;
    };
    setPos();
    window.addEventListener('resize', setPos);
    window.addEventListener('scroll', setPos, true);
    return () => {
      window.removeEventListener('resize', setPos);
      window.removeEventListener('scroll', setPos, true);
    };
  }, [open, anchorRef, entries.length]);

  // Click outside closes (but clicks inside the URL input should not).
  useEffect(() => {
    if (!open) return;
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (popRef.current?.contains(target)) return;
      if (anchorRef.current?.contains(target)) return;
      onClose();
    };
    document.addEventListener('mousedown', onMouseDown);
    return () => document.removeEventListener('mousedown', onMouseDown);
  }, [open, onClose, anchorRef]);

  // Keep selection in range when the entry list shrinks.
  useEffect(() => {
    if (entries.length === 0) return;
    if (selected < 0 || selected >= entries.length) onSelectedChange(0);
  }, [entries.length, selected, onSelectedChange]);

  if (!open) return null;

  return (
    <div
      ref={popRef}
      role="listbox"
      aria-label="环境变量"
      data-testid="env-var-picker"
      className="fixed z-50 max-h-64 overflow-y-auto rounded-md border border-border bg-popover text-popover-foreground shadow-lg animate-fade-in"
      onMouseDown={(e) => e.preventDefault() /* keep input focused */}
    >
      <div className="flex items-center gap-1.5 border-b border-border px-2 py-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
        <ChevronRight className="h-3 w-3" />
        插入环境变量{envName ? ` · ${envName}` : ''}
      </div>
      {entries.length === 0 && (
        <div className="px-3 py-6 text-center text-xs text-muted-foreground">
          没有匹配的环境变量
        </div>
      )}
      {entries.map((e, idx) => {
        const active = idx === selected;
        return (
          <button
            key={e.key}
            type="button"
            role="option"
            aria-selected={active}
            data-idx={idx}
            onMouseEnter={() => onSelectedChange(idx)}
            onClick={() => onSelect(e.key)}
            className={cn(
              'flex w-full items-center gap-2 px-2 py-1.5 text-left text-xs',
              active ? 'bg-primary/15 text-foreground' : 'hover:bg-accent/60',
            )}
          >
            <span className="shrink-0 font-mono text-blue-400">{`{{${e.key}}}`}</span>
            <span className="min-w-0 flex-1 truncate text-muted-foreground">
              {truncate(e.value, 60)}
            </span>
          </button>
        );
      })}
      <div className="flex items-center gap-3 border-t border-border bg-card/40 px-2 py-1 text-[10px] text-muted-foreground">
        <span>↑↓ 选择</span>
        <span>Tab/Enter 插入</span>
        <span>Esc 关闭</span>
      </div>
    </div>
  );
}

/** Helper for HttpTester: given the current draft URL, extract the partial
 *  variable name the user is currently typing (i.e. text after the last `{{`
 *  and before any `}}`). Returns `''` if the user isn't inside an
 *  unclosed `{{…}}` block. */
export function extractPartialVar(url: string): string {
  const lastOpen = url.lastIndexOf('{{');
  if (lastOpen < 0) return '';
  const tail = url.slice(lastOpen + 2);
  if (tail.includes('}}')) return '';
  const match = tail.match(/^([^{}\s]*)/);
  return match ? match[1] : '';
}

/** Helper for HttpTester: return true iff the user is inside an unclosed
 *  `{{…}}` block — i.e. the picker should be visible. */
export function shouldShowVarPicker(url: string): boolean {
  const lastOpen = url.lastIndexOf('{{');
  if (lastOpen < 0) return false;
  const tail = url.slice(lastOpen + 2);
  return !tail.includes('}}');
}
