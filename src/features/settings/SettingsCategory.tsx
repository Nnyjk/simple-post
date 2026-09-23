import { type ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface SettingsCategoryProps {
  /**
   * @deprecated anchor id is no longer rendered (the drawer now uses tabs).
   * Kept for API compatibility with existing call sites.
   */
  id?: string;
  title?: string;
  description?: string;
  /**
   * Optional leading icon for the section header. We pair it with the
   * title to give each category a recognisable affordance — the icon also
   * doubles as the colour anchor in the side rail, so the two views read
   * as a single source of truth.
   */
  icon?: ReactNode;
  /**
   * Trailing element in the header — typically a primary action like
   * "恢复默认" or "添加服务". Rendered as a flex child to the right of
   * the title block; reserves enough vertical room for a button without
   * inflating the row height.
   */
  actions?: ReactNode;
  /**
   * Secondary line under the description — e.g. "3 项 · 影响所有 HTTP 请求".
   * Uses the same muted type as the description, one tone cooler.
   */
  meta?: ReactNode;
  /**
   * When true, the section does not render its own bottom border. The
   * outer list (`SettingsPanel`) is expected to add a final divider
   * (or nothing) instead, so we don't end up with a stray line at the
   * bottom of the scroll area.
   */
  hideDivider?: boolean;
  /**
   * Body variant — `stacked` (default) renders children with a vertical
   * gap, `card` wraps the children in a single rounded card shell (used
   * when the section *is* one coherent surface, like the env panel).
   */
  variant?: 'stacked' | 'card';
  children: ReactNode;
}

/**
 * One entry in a settings tab detail page.
 *
 * Visually: a header (icon + small title + optional muted description)
 * followed by a content well. The negative horizontal margin pulls the
 * outer divider (when present) out to the main pane edges, while the
 * inner `px-12` keeps the body comfortably inset.
 *
 * The body is a soft card by default (`variant="card"`) so each settings
 * group reads as a single coherent surface; pass `variant="stacked"` when
 * the children are already self-contained cards or rows and only need a
 * header above them.
 */
export function SettingsCategory({
  title,
  description,
  icon,
  actions,
  meta,
  hideDivider = false,
  variant = 'card',
  children,
}: SettingsCategoryProps) {
  const hasHeader = !!title || !!description || !!icon || !!meta || !!actions;
  return (
    <section
      className={cn(
        // Outer block: pull the bottom divider out to the pane edges,
        // keep the body on a comfortable reading column. The 48px side
        // inset matches the rest of the settings tab.
        '-mx-6 px-12 py-5',
        !hideDivider && 'border-b border-border/60',
      )}
    >
      {hasHeader && (
        <header className="mb-3 flex items-start gap-3">
          {icon && (
            <span
              aria-hidden
              className="mt-0.5 inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-muted/60 text-muted-foreground"
            >
              {icon}
            </span>
          )}
          <div className="min-w-0 flex-1">
            {title && (
              <h2 className="text-sm font-semibold tracking-tight text-foreground">{title}</h2>
            )}
            {(description || meta) && (
              <p className="mt-0.5 text-[11px] text-muted-foreground">
                {description}
                {description && meta ? ' · ' : ''}
                {meta}
              </p>
            )}
          </div>
          {actions && <div className="shrink-0">{actions}</div>}
        </header>
      )}
      {variant === 'card' ? (
        <div className="rounded-lg border border-border/70 bg-card/40 shadow-[0_1px_0_0_rgba(255,255,255,0.02)_inset]">
          {children}
        </div>
      ) : (
        <div className="space-y-3">{children}</div>
      )}
    </section>
  );
}

// ----------------------------------------------------------------
// SettingRow — single key/value row inside a SettingsCategory.
//
// Layout:
//   [ icon ]  Label                        [ control ]   ← row 1
//             description text                       hint  ← row 2 (optional)
//   ──────────────────────────────────────────────────────  ← divider (optional)
//
// Use as a direct child of a `SettingsCategory` with `variant="card"` —
// each row gets its own subtle divider (top-bordered) so they read as
// a vertical list inside the card shell.
// ----------------------------------------------------------------

export interface SettingRowProps {
  /** Optional leading icon — usually the same icon used on the section
   *  header, kept small so it doesn't compete with the control. */
  icon?: ReactNode;
  label: string;
  description?: ReactNode;
  /** Right-aligned control (input / switch / select / button group). */
  control: ReactNode;
  /** Optional sub-label under the control, e.g. "默认 30000ms" or "v0.2". */
  hint?: ReactNode;
  /** Optional trailing badge, e.g. a status pill. */
  badge?: ReactNode;
  /** Visual emphasis — `danger` turns the label destructive-tinted for
   *  irreversible or removing actions. */
  tone?: 'default' | 'danger';
  /** When true, hides the row's top border (use for the first row inside
   *  a card so we don't get a stray divider at the top). */
  flush?: boolean;
  disabled?: boolean;
}

export function SettingRow({
  icon,
  label,
  description,
  control,
  hint,
  badge,
  tone = 'default',
  flush = false,
  disabled = false,
}: SettingRowProps) {
  return (
    <div
      className={cn(
        'flex items-start gap-4 px-4 py-3',
        !flush && 'border-t border-border/60 first:border-t-0',
        disabled && 'opacity-60',
      )}
    >
      {icon && (
        <span
          aria-hidden
          className={cn(
            'mt-0.5 inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md',
            tone === 'danger'
              ? 'bg-destructive/10 text-destructive'
              : 'bg-muted/60 text-muted-foreground',
          )}
        >
          {icon}
        </span>
      )}
      <div className="min-w-0 flex-1">
        <div
          className={cn(
            'text-xs font-medium',
            tone === 'danger' ? 'text-destructive' : 'text-foreground',
          )}
        >
          {label}
          {badge && <span className="ml-2 align-middle">{badge}</span>}
        </div>
        {(description || hint) && (
          <div className="mt-0.5 text-[11px] text-muted-foreground">
            {description}
            {description && hint ? (
              <>
                {' · '}
                <span className="text-muted-foreground/80">{hint}</span>
              </>
            ) : (
              hint
            )}
          </div>
        )}
      </div>
      <div className="shrink-0">{control}</div>
    </div>
  );
}
