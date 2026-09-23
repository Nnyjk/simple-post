/**
 * MethodPill — the small colored badge that represents an HTTP verb.
 *
 * Two sizes cover every place we display the HTTP method:
 *
 *   size="xs"    → 16×16 single-letter chip, used in compact rows such
 *                  as the project tree's endpoint entries. Shows just the
 *                  verb's first letter — there is no room for more. The
 *                  colour does the heavy lifting.
 *   size="sm"    → proper label-size pill that fits the full verb
 *                  ("POST" / "DELETE"). Used in the HttpTester URL bar,
 *                  dropdown rows, the dirty-close dialog, and elsewhere.
 *
 * Two visual flavours:
 *
 *   variant="solid"   → strong colour background, white text. Maximum
 *                       contrast at a glance — preferred for the active
 *                       verb in the URL bar (so it reads as the primary
 *                       "verb affordance").
 *   variant="tinted"  → the colour as a tinted background (alpha 0.15)
 *                       + coloured text. Lower visual weight, used in
 *                       dropdown rows and tree rows where a saturated
 *                       pill would shout louder than the row content.
 *
 * The same method produces the same hue across every size/variant.
 * The HSL → alpha-15 mapping lives in `methodTint()` so adjacent code
 * (e.g. dropdown row backgrounds) can reuse it without re-implementing.
 */

import type { HttpMethod } from '@/types/domain';
import { cn, methodColorVar } from '@/lib/utils';

export type MethodPillSize = 'xs' | 'sm';
export type MethodPillVariant = 'solid' | 'tinted';

export interface MethodPillProps {
  method: HttpMethod;
  /** Square single-letter vs full label. Default `sm` (full label). */
  size?: MethodPillSize;
  /** Visual flavour. Default `solid` (max contrast). */
  variant?: MethodPillVariant;
  /** Hide the text and show only the colour swatch + aria-label. */
  hideLabel?: boolean;
  className?: string;
  /**
   * Override the displayed text. Defaults to the full method (or first
   * letter when size="xs").
   */
  label?: string;
  title?: string;
}

const SIZE_XS = 'h-4 w-4 rounded text-[10px]';
const SIZE_SM = 'h-6 min-w-[3.25rem] rounded-md px-1.5 text-[10px] tracking-wider';

const VARIANT_TINTED = 'border';

export function MethodPill({
  method,
  size = 'sm',
  variant = 'solid',
  hideLabel = false,
  className,
  label,
  title,
}: MethodPillProps) {
  const text = label ?? (size === 'xs' ? method.charAt(0).toUpperCase() : method);

  // The solid variant uses the colour directly as background with white
  // text. The tinted variant uses the colour at 15% alpha as backdrop
  // and at full saturation as text — see `methodTint` below.
  const style =
    variant === 'tinted'
      ? methodTint(method)
      : { backgroundColor: methodColorVar(method), color: 'white' };

  return (
    <span
      role="img"
      aria-label={method}
      title={title ?? method}
      className={cn(
        'inline-flex shrink-0 items-center justify-center font-mono font-bold uppercase tabular-nums',
        size === 'xs' ? SIZE_XS : SIZE_SM,
        variant === 'tinted' && VARIANT_TINTED,
        className,
      )}
      style={style}
    >
      {hideLabel ? '' : text}
    </span>
  );
}

/**
 * Coloured "tinted" set: alpha-15 backdrop + full-saturation text +
 * alpha-30 border (so the pill edges remain visible on hover/focus).
 *
 * `methodColorVar` returns a modern HSL triple without commas
 * (e.g. `hsl(142 71% 45%)`). We re-emit it with an alpha channel
 * via the slash syntax — modern browsers (>= 2023) accept this in
 * any property that takes a colour.
 */
export function methodTint(method: HttpMethod): {
  backgroundColor: string;
  color: string;
  borderColor: string;
} {
  const inner = methodColorVar(method).replace(/^hsl\(|\)$/g, '');
  return {
    backgroundColor: `hsl(${inner} / 0.15)`,
    color: `hsl(${inner})`,
    borderColor: `hsl(${inner} / 0.3)`,
  };
}
