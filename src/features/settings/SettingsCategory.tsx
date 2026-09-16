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
   * When true, the section does not render its own bottom border. The
   * outer list (`SettingsPanel`) is expected to add a final divider
   * (or nothing) instead, so we don't end up with a stray line at the
   * bottom of the scroll area.
   */
  hideDivider?: boolean;
  children: ReactNode;
}

/**
 * One entry in a settings tab detail page.
 *
 * Visually: a header (small uppercase title + optional muted
 * description) sitting on top of the children, separated from the
 * previous entry by a thin horizontal divider. No card chrome, no
 * rounded corners, no background fill — the goal is to read like a
 * long form section, not a stack of boxes.
 *
 * The page-level title is rendered separately by `SettingsPanel` and
 * the outer list keeps the dividers flowing full-width via `-mx-6`
 * (so the lines reach the edges of the scroll area even though the
 * main content has its own horizontal padding).
 */
export function SettingsCategory({
  title,
  description,
  hideDivider = false,
  children,
}: SettingsCategoryProps) {
  const hasHeader = (title && title.trim().length > 0) || description;
  return (
    <section
      className={cn(
        // The negative horizontal margin pulls the divider out to the
        // main pane edges while the larger `px-` keeps the header + body
        // content comfortably inset from the divider. The 12 unit
        // inset (48px each side) gives the settings tab a noticeably
        // calmer reading column without affecting any other page.
        '-mx-6 px-12 py-4',
        !hideDivider && 'border-b border-border/60',
      )}
    >
      {hasHeader && (
        <header className="mb-3 space-y-0.5">
          {title && title.trim().length > 0 && (
            <h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
              {title}
            </h2>
          )}
          {description && (
            <p className="text-[11px] text-muted-foreground/80">{description}</p>
          )}
        </header>
      )}
      <div className="space-y-3">{children}</div>
    </section>
  );
}
