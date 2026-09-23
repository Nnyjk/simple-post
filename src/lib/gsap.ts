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

export { gsap, useGSAP, Flip };
