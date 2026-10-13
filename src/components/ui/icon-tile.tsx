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
 *   h-7   SettingsCategory / SettingRow
 *   h-9   ModuleBaseUrlTable, VariableTable
 *   h-10  ConnectionSection hero card
 *
 * …with four of them carrying a border and eleven not, and each typing
 * its own palette colour. `sm`/`md`/`lg` now name the three sizes that
 * actually recur; callers that used h-6 or h-10 move to the nearest.
 *
 * The settings rows already had two non-concept states before this
 * component existed — `concept` omitted (muted) and a destructive tint —
 * and those are now `tone="default"` and `tone="danger"`, reusing the
 * exact classes those call sites had, so nothing shifts visually.
 * `success` / `warning` / `primary` join them for tiles that report a
 * state or claim the app's primary context rather than a tree concept.
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

/**
 * Tone → paint.
 *
 * `default` is the only entry that can be overridden by `concept`; the
 * other four are states the tile is reporting rather than identities it
 * is standing for, so nothing may repaint them.
 *
 * `success` / `warning` take the `-foreground` token rather than the base
 * one. On a 15%-alpha tint the base values (`--success` 142 71% 45%,
 * `--warning` 38 92% 45%) are the *border* step of each ramp — legible as
 * an edge, too light as a glyph on the near-white the light theme puts
 * behind them. `--success-foreground` / `--warning-foreground` are the
 * text steps (30% in light, 75% in dark) and are what `Badge`'s success /
 * warning variants already use on the same tint. See the semantic-token
 * pairing note in AGENTS.md.
 *
 * `danger` deliberately does NOT follow that rule:
 * `--destructive-foreground` is `0 0% 100%` in *both* scopes, so pairing
 * it with a light tint is white-on-white in light mode. `--destructive`
 * at 5 59% 44% clears ~5:1 on that tint in both.
 */
const TONE: Record<NonNullable<IconTileProps['tone']>, { bg: string; text: string }> = {
  default: { bg: 'bg-muted/60', text: 'text-muted-foreground' },
  danger: { bg: 'bg-destructive/10', text: 'text-destructive' },
  primary: { bg: 'bg-primary/15', text: 'text-primary' },
  success: { bg: 'bg-success/15', text: 'text-success-foreground' },
  warning: { bg: 'bg-warning/15', text: 'text-warning-foreground' },
};

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
   * Paints with a semantic status token instead of a concept hue.
   *
   * `primary` exists for the two tiles that stand for the *environment* —
   * which deliberately has no hue of its own (see `src/lib/hue.ts`) and
   * keeps `--primary` because it is the app's primary context.
   *
   * `success` / `warning` / `danger` are for tiles that report the
   * *state* of a running thing. They exist because a fixed concept hue
   * cannot do that job: `concept="connection"` is rose (`--hue-connection`
   * at 355°) in every lifecycle, so a daemon's hero tile stayed red while
   * it was serving — which is a claim, not a colour preference. See
   * `src/lib/mcp-status-tone.ts`.
   */
  tone?: 'default' | 'danger' | 'primary' | 'success' | 'warning';
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
  // A concept hue wins over the neutral resting tone; every explicit tone
  // wins over both. One lookup, so no caller can end up with a hue `bg`
  // and a tone `text` — which is how a rose background once shipped with
  // a green glyph.
  const paint = hue ?? TONE[tone];
  return (
    <span
      aria-hidden={rest['aria-hidden'] ?? true}
      className={cn(
        'inline-flex shrink-0 items-center justify-center rounded-md',
        SIZE_CLASS[size],
        paint.bg,
        paint.text,
        bordered && tone === 'primary' ? PRIMARY_BORDER : bordered && hue ? hue.border : null,
        neutralBorder ? NEUTRAL_BORDER : null,
        className,
      )}
    >
      {icon}
    </span>
  );
}
