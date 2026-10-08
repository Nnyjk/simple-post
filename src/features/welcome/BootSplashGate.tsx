/**
 * Boot splash gate — tears down the HTML splash from `index.html`.
 *
 * The splash is static markup rather than a React component on purpose:
 * the single longest phase of a cold start is the entry chunk downloading
 * and executing, and *nothing* rendered from JS is visible during it.
 * Painting from HTML covers that window for free.
 *
 * That leaves the handoff. Removing the splash the instant React mounts
 * would just move the blank screen a few hundred ms later, because the
 * store still has no data — so the gate waits for the first moment the
 * app can actually show something: `loaded` (data arrived) or
 * `bootstrapError` (it never will, and the banner has to be readable).
 *
 * Mounted from `App.tsx`; it renders nothing.
 */
import { useEffect } from 'react';
import { useAppStore } from '@/stores/app-store';

export function BootSplashGate() {
  const loaded = useAppStore((s) => s.loaded);
  const bootstrapError = useAppStore((s) => s.bootstrapError);

  const done = loaded || bootstrapError !== null;

  useEffect(() => {
    if (!done) return;
    // `getElementById` per call rather than a captured ref: the node is
    // owned by index.html, and looking it up lazily keeps this correct
    // even if the document was swapped between render and effect.
    document.getElementById('boot-splash')?.remove();
  }, [done]);

  return null;
}
