import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';
import { HUE, type HueConcept } from '@/lib/hue';

/**
 * IconTile — the tinted square an icon sits in.
 *
 * This one visual idiom was open-coded at 15 call sites across five
 * different sizes:
 *
 *   h-5   project-tree rows, GroupHeader
 *   h-6   settings rail, EnvSwitcher
 *   h-7   SettingsCategory / SettingRow, EnvHeader
 *   h-9   BaseUrlTable, VariableTable
 *   h-10  ConnectionSection hero card
 *
 * …with four of them carrying a border and eleven not, and each typing
 * its own palette colour. `sm`/`md`/`lg` now name the three sizes that
 * actually recur; callers that used h-6 or h-10 move to the nearest.
 *
 * Two neutral states are supported because the settings rows already had
 * them before this component existed: `concept` omitted (muted) and
 * `tone="danger"`. Both reuse the exact classes those call sites had, so
 * nothing shifts visually.
 */

export type IconTileSize = 'xs' | 'sm' | 'md' | 'lg' | 'xl';

const SIZE_CLASS: Record<IconTileSize, string> = {
  // settings rail, env switcher: 24px
  xs: 'h-6 w-6',
  // project-tree rows, GroupHeader: 20px
  sm: 'h-5 w-5',
  // settings rows / category headers: 28px
  md: 'h-7 w-7',
  // form rows (baseUrl / variable): 36px
  lg: 'h-9 w-9',
  // ConnectionSection's hero card: 40px
  xl: 'h-10 w-10',
};

const NEUTRAL = 'bg-muted/60 text-muted-foreground';
const DANGER = 'bg-destructive/10 text-destructive';
const PRIMARY = 'bg-primary/15 text-primary';
const PRIMARY_BORDER = 'border-primary/30';
const NEUTRAL_BORDER = 'border-border';

export interface IconTileProps {
  icon: ReactNode;
  /**
   * Which semantic concept this tile stands for. Omit for a neutral tile
   * (the settings-row resting state). See `src/lib/hue.ts`.
   */
  concept?: HueConcept;
  size?: IconTileSize;
  /** Add the concept's 30%-alpha edge. Used by the form-sized rows. */
  bordered?: boolean;
  /**
   * `primary` paints with the app's `--primary` rather than a hue. It
   * exists for the two tiles that stand for the *environment* — which
   * deliberately has no hue of its own (see `src/lib/hue.ts`) and keeps
   * `--primary` because it is the app's primary context.
   */
  tone?: 'default' | 'danger' | 'primary';
  /** Neutral tiles on the settings rail also carry a hairline border. */
  neutralBorder?: boolean;
  className?: string;
  /** Mirrored onto the wrapper for the "hide from AT" call sites. */
  'aria-hidden'?: boolean;
}

export function IconTile({
  icon,
  concept,
  size = 'md',
  bordered = false,
  tone = 'default',
  neutralBorder = false,
  className,
  ...rest
}: IconTileProps) {
  const hue = tone === 'default' && concept ? HUE[concept] : null;
  return (
    <span
      aria-hidden={rest['aria-hidden'] ?? true}
      className={cn(
        'inline-flex shrink-0 items-center justify-center rounded-md',
        SIZE_CLASS[size],
        tone === 'danger' ? DANGER : tone === 'primary' ? PRIMARY : hue ? hue.bg : NEUTRAL,
        tone === 'default' ? (hue ? hue.text : null) : null,
        bordered && tone === 'primary' ? PRIMARY_BORDER : bordered && hue ? hue.border : null,
        neutralBorder ? NEUTRAL_BORDER : null,
        className,
      )}
    >
      {icon}
    </span>
  );
}
