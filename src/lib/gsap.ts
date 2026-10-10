/**
 * Single GSAP init point for the renderer.
 *
 * Why this file exists:
 *  - `gsap.registerPlugin(...)` must run once, before any animation code
 *    executes. Centralising it here means every component just imports
 *    `gsap` / `useGSAP` / `Flip` from this module and gets the
 *    registrations for free — no chance of forgetting to register, no
 *    duplicate registration, no ordering surprises in dev.
 *  - Keeping the gsap + @gsap/react packages off the rest of the
 *    codebase's import surface also makes it easy to swap or remove
 *    later (delete this file + the deps; nothing else breaks).
 *
 * IMPORTANT: do not import this file from anywhere that runs during
 * SSR. This is a Tauri 2 webview app, so SSR is not in play — but if
 * that ever changes, gate the import on a `typeof window !== 'undefined'`
 * check.
 */
import gsap from 'gsap';
import { useGSAP } from '@gsap/react';
import { Flip } from 'gsap/Flip';

// Register once at module evaluation. Plugins are idempotent — calling
// `registerPlugin` twice is a no-op — but doing it here keeps every
// consumer's mental model simple: "I import from '@/lib/gsap', I get
// fully wired plugins."
gsap.registerPlugin(useGSAP, Flip);

/**
 * Run an animation only when the user has not asked the OS for reduced
 * motion. Returns a cleanup function, so it composes with `useGSAP`'s own
 * return-a-cleanup contract:
 *
 * ```ts
 * useGSAP(() => {
 *   if (!panelRef.current) return;
 *   return motionSafe(() => {
 *     gsap.fromTo(panelRef.current, { opacity: 0 }, { opacity: 1 });
 *   });
 * }, [open]);
 * ```
 *
 * `jumpToEnd` is for animations that are **carrying state**, not
 * decorating it — pass what should happen with no motion at all:
 *
 * ```ts
 * return motionSafe(() => gsap.to(el, { height: 0 }), () => gsap.set(el, { height: 0 }));
 * ```
 *
 * This distinction is the whole point, and getting it wrong breaks the
 * feature rather than merely looking different. A collapsed tree branch
 * is held closed by an inline `height: 0` that GSAP owns, so simply not
 * running the expand tween leaves the branch invisible after the user
 * expanded it. Decoration can be skipped; state has to land somewhere.
 *
 * Why a helper instead of `gsap.matchMedia()` at each call site:
 * `matchMedia` is the right primitive, but the useful part is the
 * `(prefers-reduced-motion: …)` strings and the `revert()`, and both are
 * easy to leave off. This code base had exactly one hand-written
 * `matchMedia` (the body editor's validation pill) beside seven
 * unguarded `useGSAP` blocks, which reads as an oversight rather than a
 * policy. One spelling makes "we respect this" true everywhere or
 * visibly false everywhere.
 *
 * Note the queries are `no-preference` / `reduce`, not one-sided: the
 * default is to animate and the opt-out is explicit, and a browser that
 * understands neither runs nothing rather than animating unconditionally.
 */
export function motionSafe(animate: () => void, jumpToEnd?: () => void): () => void {
  const mm = gsap.matchMedia();
  mm.add('(prefers-reduced-motion: no-preference)', animate);
  if (jumpToEnd) mm.add('(prefers-reduced-motion: reduce)', jumpToEnd);
  return () => mm.revert();
}

export { gsap, useGSAP, Flip };
