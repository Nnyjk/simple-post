import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

/**
 * Shared chrome used by every settings-style panel (entity settings,
 * future workspace panes, etc.). Kept here so all surfaces render
 * section dividers, field labels, and empty/not-found hints the same
 * way — change once, updates everywhere.
 */

/**
 * A divider section with a small uppercase title bar and an optional
 * right-aligned action slot (count, "+ 新建" button, etc.).
 */
export function Section({
  title,
  right,
  children,
}: {
  title: string;
  right?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="border-b border-border/60 px-5 py-3">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
          {title}
        </h2>
        {right}
      </div>
      <div className="space-y-3">{children}</div>
    </div>
  );
}

/**
 * A labelled form field. `label` renders above; an optional `hint`
 * renders below as muted helper text.
 */
export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <label className="block max-w-2xl space-y-1">
      <span className="text-[11px] font-medium text-muted-foreground">{label}</span>
      {children}
      {hint && <span className="block text-[10px] text-muted-foreground/70">{hint}</span>}
    </label>
  );
}

/**
 * Inline placeholder shown when a list is empty. Uses a dashed border
 * to read as "this is where content will go", not as an alert.
 */
export function EmptyHint({ text }: { text: string }) {
  return (
    <div className="rounded-md border border-dashed border-border/60 px-3 py-4 text-center text-[11px] text-muted-foreground">
      {text}
    </div>
  );
}

/**
 * Centered placeholder for when an entity has been deleted out from
 * under the open tab. Matches the panel height so the layout doesn't
 * jump.
 */
export function NotFound({ label }: { label: string }) {
  return (
    <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
      {label}已被删除。
    </div>
  );
}

/**
 * Default project-color swatch palette. Picked to be legible on both
 * light and dark card backgrounds and to avoid clashing with the
 * method colors used elsewhere in the tree.
 */
const DEFAULT_PROJECT_COLORS = [
  '#60a5fa',
  '#a78bfa',
  '#34d399',
  '#fbbf24',
  '#f87171',
  '#a3a3a3',
];

/**
 * Round color swatch row for picking a single accent color. The
 * selected swatch gets a focus-ring style so the chosen state is
 * obvious in both light and dark themes.
 */
export function ColorRow({
  value,
  onChange,
  colors = DEFAULT_PROJECT_COLORS,
}: {
  value: string;
  onChange: (c: string) => void;
  colors?: readonly string[];
}) {
  return (
    <div className="flex items-center gap-1.5">
      {colors.map((c) => (
        <button
          key={c}
          type="button"
          onClick={() => onChange(c)}
          aria-label={`选择颜色 ${c}`}
          className={cn(
            'h-5 w-5 rounded-full transition-all',
            value === c
              ? 'ring-2 ring-ring ring-offset-2 ring-offset-card'
              : 'opacity-70 hover:opacity-100',
          )}
          style={{ backgroundColor: c }}
        />
      ))}
    </div>
  );
}