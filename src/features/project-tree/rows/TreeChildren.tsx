import { useRef, type ReactNode } from 'react';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';

/**
 * Wraps the children of a tree node (module → CollectionBranch, or
 * CollectionRow → endpoints + sub-collections) so we can animate the
 * expand/collapse with GSAP instead of a hard mount/unmount. The
 * children stay in the DOM at all times; visual presence is driven by
 * the container's `height` + `opacity`. On expand we also stagger-fade
 * the direct row descendants so the new content reveals itself
 * row-by-row rather than popping in all at once.
 */
export function TreeChildren({ expanded, children }: { expanded: boolean; children: ReactNode }) {
  const containerRef = useRef<HTMLDivElement>(null);
  // `useGSAP` runs the callback once on mount in addition to every
  // dependency change. Without a guard, an initially-expanded branch
  // would tween from height: 0 → 'auto' on first paint and flash from
  // collapsed. We sync to the current state on the first run and
  // animate on every subsequent change.
  const isFirstRun = useRef(true);

  useGSAP(
    () => {
      const el = containerRef.current;
      if (!el) return;
      if (isFirstRun.current) {
        isFirstRun.current = false;
        gsap.set(el, expanded ? { height: 'auto', opacity: 1 } : { height: 0, opacity: 0 });
        return;
      }
      // The transition is motion, but it is also the only thing that moves
      // this container between open and closed, so reduced motion has to
      // land on the end state rather than skip it — the collapsed height
      // is an inline style GSAP owns, and a skipped tween would leave a
      // branch the user just expanded sitting at `height: 0`.
      const rows = el.querySelectorAll<HTMLElement>(':scope > * > [data-tree-row]');
      if (expanded) {
        return motionSafe(
          () => {
            gsap.fromTo(
              el,
              { height: 0, opacity: 0 },
              {
                height: 'auto',
                opacity: 1,
                duration: 0.25,
                ease: 'power2.out',
              },
            );
            gsap.from(rows, {
              opacity: 0,
              y: 4,
              duration: 0.2,
              stagger: 0.025,
              ease: 'power2.out',
            });
          },
          () => {
            gsap.set(el, { height: 'auto', opacity: 1 });
            gsap.set(rows, { opacity: 1, y: 0 });
          },
        );
      }
      return motionSafe(
        () => {
          gsap.to(el, {
            height: 0,
            opacity: 0,
            duration: 0.2,
            ease: 'power2.in',
          });
        },
        () => gsap.set(el, { height: 0, opacity: 0 }),
      );
    },
    { dependencies: [expanded], scope: containerRef },
  );

  return (
    <div ref={containerRef} style={{ overflow: 'hidden' }} aria-hidden={!expanded}>
      {children}
    </div>
  );
}
