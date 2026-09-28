import type { CSSProperties } from 'react';
import type { DropZone } from '../tree-types';

/**
 * Visual indicator for the current drop position on a row. Drawn as a
 * 2px line for `above` / `below` and a colored ring for `into` (the
 * row itself already tints its background in that case).
 */
export function DropHintOverlay({ zone }: { zone: DropZone | null }) {
  if (!zone) return null;
  const style: CSSProperties =
    zone === 'above'
      ? { top: -1, left: 4, right: 4, height: 2 }
      : zone === 'below'
        ? { bottom: -1, left: 4, right: 4, height: 2 }
        : { display: 'none' }; // 'into' is drawn by the row's bg+ring
  return (
    <div aria-hidden className="pointer-events-none absolute rounded-sm bg-primary" style={style} />
  );
}
