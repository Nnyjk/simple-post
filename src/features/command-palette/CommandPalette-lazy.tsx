/**
 * Lazy boundary for the command palette (Ctrl+K).
 *
 * The palette is mounted unconditionally by `TopBar` but is only ever
 * visible in response to a keystroke, so keeping it in the entry graph
 * meant every cold start paid for a dialog almost nobody opens on load.
 *
 * `TopBar` renders it behind a `Suspense` with a `null` fallback: there is
 * nothing to show until `open` flips to true, and by then the user has
 * already pressed a key, so the chunk is normally resident or one fetch away.
 */
import { lazy } from 'react';

const CommandPaletteImpl = lazy(() =>
  import('./CommandPalette').then((m) => ({ default: m.CommandPalette })),
);

export function CommandPalette(props: { open: boolean; onClose: () => void }) {
  return <CommandPaletteImpl {...props} />;
}
