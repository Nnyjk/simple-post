import { type ReactNode } from 'react';
import { cn, focusRing } from '@/lib/utils';

export interface TabItem {
  id: string;
  label: ReactNode;
  badge?: ReactNode;
  disabled?: boolean;
}

interface TabsBaseProps {
  items: TabItem[];
  value: string;
  onChange: (id: string) => void;
  className?: string;
  size?: 'sm' | 'md';
}

export type TabsProps =
  | (TabsBaseProps & { variant?: 'underline'; joinStripClass?: never })
  | (TabsBaseProps & {
      variant: 'connected';
      /**
       * The strip's own background — the same classes the element holding
       * this `Tabs` uses, and the same tint the panel behind it uses.
       *
       * Required rather than optional because the corner carve is painted
       * in it: get it wrong and the tab's shoulders show a colour that
       * matches nothing around them. It only has to agree with the strip,
       * and only the strip.
       */
      joinStripClass: string;
    });

/**
 * Underline-style tab bar — the same visual idiom used by the workspace
 * tab strip at the top of the app, but stripped of FLIP animation and
 * the close affordance. We use a `border-b-2` underline on the active
 * tab instead of a background pill so it reads as a navigation surface
 * (rather than a segmented control) and so it lines up with the rest
 * of the chrome at the bottom of the strip.
 *
 * `variant="connected"` swaps that for the browser-tab idiom: the active
 * tab is filled with the panel's own background and sits *on top of* the
 * separator, so it reads as part of the panel below rather than as a
 * control floating above it. See `TabJoinCorners` for the shoulders.
 */
export function Tabs({ items, value, onChange, className, size = 'sm', ...rest }: TabsProps) {
  const connected = rest.variant === 'connected';
  const joinStripClass = rest.variant === 'connected' ? rest.joinStripClass : null;

  return (
    <div
      role="tablist"
      className={cn(
        // The strip itself carries a hairline separator at the bottom so
        // an active tab's underline can sit *above* it (using -mb-px on
        // the tab) and look like it's growing out of the surface — the
        // pattern you see in VS Code, browsers, GitHub's file tabs.
        //
        // `connected` has no line of its own: the element wrapping the
        // strip owns the full-width separator (the strip is usually only
        // as wide as its labels, and a line that stopped short would read
        // as a mistake), and the active tab covers it. That covering only
        // works while the wrapper does *not* clip — `overflow-hidden`
        // clips to the padding box, which is above the wrapper's border.
        connected ? 'flex items-end gap-0' : 'flex items-end gap-0 border-b border-border/60',
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
              'relative inline-flex items-center gap-1.5 px-3 font-medium transition-colors',
              focusRing,
              'disabled:pointer-events-none disabled:opacity-50',
              size === 'sm' ? 'h-8 text-xs' : 'h-9 text-sm',
              // The `-mb-px` lifts the tab one pixel onto the separator so
              // the two collapse into one continuous stroke — under
              // `underline` the underline is the stroke, under
              // `connected` the tab's own background is.
              '-mb-px',
              connected
                ? cn(
                    'rounded-t-md',
                    // `z-10` puts the active fill above the separator it
                    // is meant to erase; inactive tabs stay unmarked so the
                    // line runs past them unbroken.
                    active
                      ? 'z-10 bg-background text-foreground'
                      : 'text-muted-foreground hover:bg-accent/40 hover:text-foreground',
                  )
                : cn(
                    'border-b-2',
                    active
                      ? 'border-primary text-foreground'
                      : 'border-transparent text-muted-foreground hover:border-border/60 hover:text-foreground',
                  ),
            )}
          >
            {item.label}
            {item.badge}
            {active && joinStripClass && <TabJoinCorners stripClass={joinStripClass} />}
          </button>
        );
      })}
    </div>
  );
}

/** Corner carve radius. */
const JOIN_R = 'h-2.5 w-2.5';
/** A 20px circle parked on the corner's *inner* point, clipped to the window. */
const JOIN_DISC = 'absolute h-5 w-5 rounded-full';

/**
 * The two corner carves that make an active tab read as *merged* with the
 * panel below it.
 *
 * A separator line that simply stops at the tab's sides leaves the tab
 * looking like a card parked on top of the page. Browsers instead round
 * the tab's shoulders into the line, so the outline never breaks at the
 * junction — that rounding is the whole "one piece" effect, and it has to
 * be drawn in the *strip's* colour rather than left to the fill, because
 * a light-theme strip is often the same colour as the panel and only the
 * outline tells the two apart.
 *
 * Each side is a zero-overflow `JOIN_R` window at the tab's bottom corner
 * with a `JOIN_DISC` circle centred on that corner: the circle's quarter
 * inside the window is painted in the strip colour, which erases the
 * tab's fill *and* its side border over a quarter disc — a concave bite,
 * so the border ends on a curve instead of a right angle.
 *
 * The circle has to be clipped to the window (rather than simply parked
 * on the corner) because its outer three quarters would otherwise spill
 * past the tab and paint a visible half-disc into the panel below.
 */
export function TabJoinCorners({ stripClass }: { stripClass: string }) {
  return (
    <>
      <span
        aria-hidden
        className={cn('pointer-events-none absolute bottom-0 left-0 overflow-hidden', JOIN_R)}
      >
        <span className={cn(JOIN_DISC, '-bottom-2.5 -left-2.5', stripClass)} />
      </span>
      <span
        aria-hidden
        className={cn('pointer-events-none absolute bottom-0 right-0 overflow-hidden', JOIN_R)}
      >
        <span className={cn(JOIN_DISC, '-bottom-2.5 -right-2.5', stripClass)} />
      </span>
    </>
  );
}
