import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

/**
 * EmptyHint — inline placeholder shown when a list is empty.
 *
 * Uses a dashed border to read as "this is where content will go", not as
 * an alert.
 *
 * The default box metrics match the original `section.tsx` treatment. Two
 * call sites need a different density (a taller box in the env panel, a
 * borderless one in the shortcuts list), so `className` merges over the
 * defaults — callers override only the axis that differs and keep the
 * dashed-border vocabulary.
 */
export function EmptyHint({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div
      className={cn(
        'rounded-md border border-dashed border-border/60 px-3 py-4 text-center text-[11px] text-muted-foreground',
        className,
      )}
    >
      {children}
    </div>
  );
}
