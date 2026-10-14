import type { CSSProperties } from 'react';
import type { DropZone } from '../tree-types';

/**
 * 行上当前放置位置的可视指示。`above` / `below` 画成 2px 横线；`into` 由该行自身的
 * 背景染色与描边表示，故此处不画。
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
