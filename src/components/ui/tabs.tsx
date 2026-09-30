import { type ReactNode } from 'react';
import { cn, focusRing } from '@/lib/utils';

export interface TabItem {
  id: string;
  label: ReactNode;
  badge?: ReactNode;
  disabled?: boolean;
}

interface TabsProps {
  items: TabItem[];
  value: string;
  onChange: (id: string) => void;
  className?: string;
  size?: 'sm' | 'md';
}

/**
 * Underline-style tab bar — the same visual idiom used by the workspace
 * tab strip at the top of the app, but stripped of FLIP animation and
 * the close affordance. We use a `border-b-2` underline on the active
 * tab instead of a background pill so it reads as a navigation surface
 * (rather than a segmented control) and so it lines up with the rest
 * of the chrome at the bottom of the strip.
 */
export function Tabs({ items, value, onChange, className, size = 'sm' }: TabsProps) {
  return (
    <div
      role="tablist"
      className={cn(
        // The strip itself carries a hairline separator at the bottom so
        // an active tab's underline can sit *above* it (using -mb-px on
        // the tab) and look like it's growing out of the surface — the
        // pattern you see in VS Code, browsers, GitHub's file tabs.
        'flex items-end gap-0 border-b border-border/60',
        className,
      )}
    >
      {items.map((item) => {
        const active = item.id === value;
        return (
          <button
            key={item.id}
            role="tab"
            aria-selected={active}
            disabled={item.disabled}
            onClick={() => onChange(item.id)}
            className={cn(
              'relative inline-flex items-center gap-1.5 border-b-2 px-3 font-medium transition-colors',
              focusRing,
              'disabled:pointer-events-none disabled:opacity-50',
              size === 'sm' ? 'h-8 text-xs' : 'h-9 text-sm',
              // The `-mb-px` lifts the underline onto the strip's border
              // so the two lines collapse into one continuous stroke.
              '-mb-px',
              active
                ? 'border-primary text-foreground'
                : 'border-transparent text-muted-foreground hover:border-border/60 hover:text-foreground',
            )}
          >
            {item.label}
            {item.badge}
          </button>
        );
      })}
    </div>
  );
}
