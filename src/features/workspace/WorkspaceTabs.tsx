/**
 * WorkspaceTabs — the horizontal tab strip rendered above the right pane.
 *
 * Each tab represents one entity (project / module / collection / endpoint)
 * the user has opened. Clicking activates; the small "×" closes.
 *
 * Behavioural details worth knowing:
 *   - Tabs whose underlying entity was deleted (or whose parent was) are
 *     auto-pruned on every state change. The cleanup is also exposed as
 *     `pruneStaleTabs` for the rare case of out-of-band mutations.
 *   - When there are more tabs than fit in the available width, the strip
 *     enters a **compact mode** instead of showing a horizontal scrollbar
 *     (which would have clobbered the rest of the chrome). Both modes use
 *     **fixed widths** (normal=160px, compact=92px) — every tab in the
 *     strip is the same width regardless of what it shows, so the strip
 *     reads as a clean grid. The title inside each tab truncates to
 *     ≈ 5–6 Chinese characters in normal mode and ≈ 2 in compact; the
 *     hint (e.g. "订单生命周期") is shown in normal mode but capped via
 *     `max-w` and hidden in compact. Width transitions are CSS-driven so
 *     neighbours reflow smoothly; tab *positions* still slide via GSAP
 *     FLIP. Re-measured whenever the tab set, active tab, or window size
 *     changes.
 *   - Keyboard: middle-click closes, just like a browser tab.
 *   - Visual state: the active tab merges into the panel below (browser
 *     idiom — see `TabJoinCorners`), and its label is the only one on the
 *     strip at DemiBold weight. WinShun's `PageTabs.qml` steps weight
 *     rather than fill to mark the current item; that transfers here
 *     without its width-reservation hack, because these tabs are a fixed
 *     160/92px and the labels truncate rather than reflow.
 *   - Tab positions slide rather than snap when the tab set changes
 *     (add / remove / reorder). This is implemented as a FLIP animation
 *     via GSAP Flip — see the useGSAP block below.
 *   - The 0.5px primary stripe under the active tab is a single
 *     persistent element at the strip level. Its geometry is *derived*
 *     from the active tab's live rect on every animation frame while any
 *     layout animation can be in flight, rather than measured once and
 *     tweened to — see "Active-tab indicator" below for why measuring
 *     once is what made it drift and stretch when tabs were closed.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import {
  Package,
  FolderOpen,
  FolderClosed,
  X,
  Hash,
  ScrollText,
  XSquare,
  CheckCircle2,
  Settings2,
} from 'lucide-react';
import { useAppStore, type OpenTab, type TabKind } from '@/stores/app-store';
import { Button } from '@/components/ui/button';
import { TabJoinCorners } from '@/components/ui/tabs';
import { Tooltip } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { MethodPill } from '@/components/ui/method-pill';
import { HUE } from '@/lib/hue';
import { Flip, gsap, useGSAP } from '@/lib/gsap';

const KIND_LABEL: Record<TabKind, string> = {
  project: '项目',
  module: '模块',
  collection: '集合',
  endpoint: '接口',
  // A draft endpoint has no row in `nodes` — it is renderer state until
  // Save writes it — so if it ever reaches `missingDescriptor` the only
  // honest label is "unsaved", not "deleted".
  'endpoint-draft': '未保存',
  settings: '设置',
};

/**
 * Active-tab indicator tuning. See the "Active-tab indicator" block in
 * the component for why these exist at all.
 *
 * `BAR_APPROACH` is the per-frame fraction of the remaining distance the
 * bar closes on the active tab. At 60fps, 0.24 leaves ~1% of the gap
 * after 17 frames — i.e. it settles in about the 0.28s the Flip tween
 * runs for, which is what makes the two feel like one motion.
 *
 * `BAR_SETTLE_MS` is how long the driver keeps reading the live rect
 * after something changes. It has to outlast the longest animation that
 * can move a tab: the 280ms Flip plus the 200ms CSS width transition,
 * with room for a resize to trigger both back to back.
 */
const BAR_APPROACH = 0.24;
const BAR_SETTLE_MS = 480;

interface TabDescriptor {
  tab: OpenTab;
  title: string;
  hint?: string;
  icon: React.ReactNode;
  /**
   * Text colour class for the leading icon, so the strip stays scannable
   * and matches the tree row for the same entity.
   *
   * This was an inline `style={{ color: … }}` with a hand-typed hex
   * before. Two problems: the hex had already drifted from the tree's
   * own colour for the same concept, and a fixed hex can't follow the
   * theme — the module icon stayed amber-400 in dark mode while the tree
   * row had already shifted. A class backed by a CSS variable fixes both.
   */
  iconColor: string;
}

export function WorkspaceTabs() {
  const openTabs = useAppStore((s) => s.openTabs);
  const activeTabId = useAppStore((s) => s.activeTabId);
  const dirtyTabIds = useAppStore((s) => s.dirtyTabIds);
  const setActiveTab = useAppStore((s) => s.setActiveTab);
  const requestCloseTab = useAppStore((s) => s.requestCloseTab);
  const requestCloseOthers = useAppStore((s) => s.requestCloseOthers);
  const requestCloseAllTabs = useAppStore((s) => s.requestCloseAllTabs);
  const closeSavedTabs = useAppStore((s) => s.closeSavedTabs);
  const pruneStaleTabs = useAppStore((s) => s.pruneStaleTabs);

  // Right-click context menu — anchored to the tab the user clicked.
  // We render a small floating menu at the click position; closing is
  // handled by outside click / Esc.
  const [menu, setMenu] = useState<{ tabId: string; x: number; y: number } | null>(null);

  // Cleanup safety net: covers any mutation path that bypassed the
  // delete* actions (e.g. imported data, future Rust sync). Cheap because
  // the data is bounded by the workspace size.
  //
  // `nodesLoaded` is a dependency, not an incidental capture: `pruneStaleTabs`
  // no-ops until the tree has loaded (an empty `nodes` array is
  // indistinguishable from "all deleted"), so without re-running on that
  // flip the tabs of a freshly-entered project would never be pruned.
  const nodesLoaded = useAppStore((s) => s.nodesLoaded);
  useEffect(() => {
    pruneStaleTabs();
  }, [pruneStaleTabs, nodesLoaded]);

  // Build a descriptor for every tab. Done locally with useMemo so each
  // tab only re-renders when its underlying entity changes.
  const descriptors = useTabDescriptors(openTabs);

  // Keep the active tab in view when the user adds many tabs.
  const stripRef = useRef<HTMLDivElement>(null);

  // Compact-mode decision: when the strip cannot host every tab at its
  // full width, we let each tab shrink to a smaller floor (icon + 4-5
  // chars + close) instead of showing a horizontal scrollbar — which
  // would clash with the rest of the chrome. The title inside the tab
  // truncates with `max-w`; it is never hidden behind an icon-only
  // treatment (the eye should always see *some* text in a tab).
  //
  // Both modes now use FIXED widths (normal=160px, compact=92px) so
  // every tab in the strip looks the same — the user asked for uniform
  // tab widths to keep the strip visually consistent. The compact mode
  // threshold counts how many *normal* tabs would fit, then collapses
  // everyone to the compact width if not.
  //
  // Re-measured whenever the tab set changes, the active tab switches,
  // or the window resizes (we listen for resize rather than running a
  // ResizeObserver because the only thing we need is the strip's
  // own clientWidth, and resize fires at most once per frame).
  const [isCompact, setIsCompact] = useState(false);
  const measureCompact = useCallback(() => {
    const el = stripRef.current;
    if (!el) return;
    const NORMAL_TAB = 160; // px, fixed width in normal mode
    const TRAILING = 76; // px reserved for the trailing "关闭其他" button + padding
    const tabs = el.querySelectorAll<HTMLElement>('[data-tab-id]');
    const minTotal = tabs.length * NORMAL_TAB;
    setIsCompact(minTotal > el.clientWidth - TRAILING);
  }, []);
  useLayoutEffect(() => {
    measureCompact();
  }, [measureCompact, activeTabId, openTabs.length]);
  useEffect(() => {
    window.addEventListener('resize', measureCompact);
    return () => window.removeEventListener('resize', measureCompact);
  }, [measureCompact]);

  // ------------------------------------------------------------------
  //  Tab position animation (GSAP Flip, FLIP pattern)
  // ------------------------------------------------------------------
  // useGSAP's `dependencies` + `revertOnUpdate: true` runs each callback
  // *after* React commits, then registers a cleanup that's invoked the
  // next time deps change. We capture tab positions in the callback and
  // play them back in the cleanup — that order is what gives us FLIP:
  // the new DOM has already been committed by the time the cleanup runs,
  // so `Flip.from(state)` can tween from the previously-captured positions
  // to the just-committed positions. `absolute: true` lets Flip take the
  // tabs out of flow during the tween so neighbours don't reflow as each
  // tab slides.
  useGSAP(
    () => {
      if (!stripRef.current) return;
      const targets = stripRef.current.querySelectorAll<HTMLElement>('[data-tab-id]');
      if (targets.length === 0) return;
      const state = Flip.getState(targets);
      return () => {
        Flip.from(state, {
          duration: 0.28,
          ease: 'power2.out',
          absolute: true,
        });
      };
    },
    { dependencies: [openTabs], scope: stripRef, revertOnUpdate: true },
  );

  // ------------------------------------------------------------------
  //  Active-tab indicator
  // ------------------------------------------------------------------
  // DERIVED EVERY FRAME, NOT MEASURED ONCE.
  //
  // Three separate things move these tabs, and each of them can still be
  // in flight at the moment an old "measure the active tab, tween the bar
  // to that number" implementation would have read its target:
  //
  //   1. The Flip tween above. With `absolute: true` it re-parents every
  //      tweened tab to `position: absolute` at its *pre-commit* spot, so
  //      an `offsetLeft` read during the tween is the animated position,
  //      not the final one. Reading it and tweening to it pointed the bar
  //      at where the tab used to be.
  //   2. The CSS width transition on the tab (`transition-[width,…]`,
  //      200ms) when the strip flips between normal and compact width.
  //      That changes every tab's width *and* every neighbour's offset
  //      for the whole transition — and it was not in the bar's
  //      dependency list at all, so the bar kept a stale width after
  //      closing tabs took the strip out of compact mode.
  //   3. The strip's own `scrollLeft` (the wheel handler and
  //      `scrollIntoView` both move it).
  //
  // So the bar's target is re-read from the live rect on every frame
  // while any of those can be running. Because the *current* position is
  // then eased toward that target with a fixed per-frame factor, the bar
  // both slides between tabs and tracks them mid-animation, and it cannot
  // end up parked on a number that has since gone stale.
  //
  // The loop is not permanent: it re-runs for a bounded window whenever
  // something that can move a tab changes (active tab, tab set, compact
  // mode) and stops once the window elapses. Long enough to cover the
  // 280ms Flip and the 200ms width transition, plus a resize burst on
  // top of both; short enough that an idle strip does no per-frame work.
  const barRef = useRef<HTMLSpanElement>(null);
  // `started` distinguishes "place it" from "move it": the first frame
  // for a given activation snaps (there is no previous position worth
  // animating from — a fresh mount, or the strip coming back from having
  // no tabs at all), and every frame after that eases.
  const barState = useRef({ x: 0, width: 0, started: false });

  const placeBar = useCallback(() => {
    const strip = stripRef.current;
    const bar = barRef.current;
    if (!strip || !bar) {
      // The strip itself unmounts when the last tab closes
      // (`descriptors.length === 0` returns null above). Forgetting the
      // previous position here means the next strip that appears snaps
      // onto its tab instead of sliding in from a stale coordinate.
      barState.current.started = false;
      return;
    }
    const activeEl = activeTabId
      ? strip.querySelector<HTMLElement>(`[data-tab-id="${activeTabId}"]`)
      : null;
    if (!activeEl) {
      // Nothing is active (every tab closed) — collapse the bar rather
      // than leave it parked under a tab that is no longer there.
      barState.current.started = false;
      gsap.set(bar, { width: 0, opacity: 0 });
      return;
    }
    const stripRect = strip.getBoundingClientRect();
    const tabRect = activeEl.getBoundingClientRect();
    // The bar is absolutely positioned *inside* the scroll container, so it
    // is laid out in content coordinates — the scroll offset has to be
    // added back, or the bar drifts by exactly the amount scrolled.
    const targetX = tabRect.left - stripRect.left + strip.scrollLeft;
    const targetW = tabRect.width;
    const cur = barState.current;
    if (cur.started) {
      // Exponential approach: ~0.28s to settle, which is what the removed
      // 0.28s `power2.out` tween felt like, but toward a target that may
      // itself be moving.
      cur.x += (targetX - cur.x) * BAR_APPROACH;
      cur.width += (targetW - cur.width) * BAR_APPROACH;
      if (Math.abs(targetX - cur.x) < 0.25 && Math.abs(targetW - cur.width) < 0.25) {
        cur.x = targetX;
        cur.width = targetW;
      }
    } else {
      cur.x = targetX;
      cur.width = targetW;
      cur.started = true;
    }
    gsap.set(bar, { x: cur.x, width: cur.width, opacity: 1 });
  }, [activeTabId]);

  useEffect(() => {
    let raf = 0;
    const startedAt = performance.now();
    const tick = (now: number) => {
      placeBar();
      if (now - startedAt < BAR_SETTLE_MS) raf = requestAnimationFrame(tick);
    };
    // First frame runs on the next animation frame, not synchronously:
    // that is the first moment the Flip tween has actually started, so
    // the bar reads a position that is real rather than a committed one
    // it is about to animate away from.
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
    // `openTabs` (not just its length) and `isCompact` are both listed:
    // removing a tab that is *not* the active one leaves `activeTabId`
    // untouched while every tab to its right slides left, and a compact
    // flip resizes them all.
  }, [placeBar, openTabs, isCompact]);

  useEffect(() => {
    if (!activeTabId) return;
    const el = stripRef.current?.querySelector<HTMLElement>(`[data-tab-id="${activeTabId}"]`);
    if (el) el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }, [activeTabId, openTabs.length]);

  // Convert vertical wheel to horizontal scroll for the tab strip — matches
  // every browser's tab bar behaviour.
  useEffect(() => {
    const el = stripRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (e.deltaY === 0 || Math.abs(e.deltaY) < Math.abs(e.deltaX)) return;
      if (el.scrollWidth <= el.clientWidth) return;
      e.preventDefault();
      el.scrollLeft += e.deltaY;
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, []);

  if (descriptors.length === 0) return null;

  return (
    <div
      ref={stripRef}
      role="tablist"
      aria-label="工作区标签"
      className={cn(
        // No `border-b` here. The separator is a child (`below`) rather
        // than a border because this strip is `overflow-hidden`, and that
        // clips to the *padding* box — a border of its own would sit
        // outside the clip and no descendant could ever cover it, so an
        // active tab could not merge with the panel below. A 1px child
        // line lives inside the clip, where the tab's fill can erase it.
        'relative flex h-9 shrink-0 items-stretch gap-0.5 overflow-hidden bg-card/40 pl-0 pr-1.5',
        // Belt-and-braces: hide both the standard scrollbar and the
        // webkit one. The strip itself is `overflow-hidden` so a scrollbar
        // wouldn't actually appear here — but keeping the rules in case a
        // future change ever reintroduces `overflow-x-auto`.
        'scrollbar-none',
      )}
    >
      {/* The separator. `z-0` so the active tab's fill (z-10) wins over
          the stretch of line underneath it. */}
      <span
        aria-hidden
        className="pointer-events-none absolute inset-x-0 bottom-0 z-0 h-px bg-border"
      />
      {descriptors.map((d) => {
        const active = d.tab.id === activeTabId;
        const dirty = dirtyTabIds.has(d.tab.id);
        return (
          <div
            key={`${d.tab.kind}:${d.tab.id}`}
            data-tab-id={d.tab.id}
            role="tab"
            aria-selected={active}
            onMouseDown={(e) => {
              // Middle-click closes the tab (browser convention).
              if (e.button === 1) {
                e.preventDefault();
                requestCloseTab(d.tab.id);
              }
            }}
            onAuxClick={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                requestCloseTab(d.tab.id);
              }
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              setMenu({ tabId: d.tab.id, x: e.clientX, y: e.clientY });
            }}
            className={cn(
              // Width strategy (uniform across all tabs in a session):
              //  - normal mode: every tab is exactly `w-[160px]` so the
              //    strip reads as a clean grid of equal-width cells; the
              //    user explicitly asked for "统一长度". Title fits 5–6
              //    Chinese characters before truncating; long hints are
              //    truncated with `...` inside the tab rather than
              //    stretching the tab outward.
              //  - compact mode (triggered when too many tabs to fit at
              //    160px): every tab is exactly `w-[92px]`. The title
              //    truncates to ≈ 2 Chinese characters so the active tab
              //    is identifiable at a glance. The hint is hidden
              //    (otherwise its `shrink-0` would visually break the
              //    fixed 92px width).
              'group relative flex h-full shrink-0 cursor-pointer items-center gap-1.5 rounded-t-md border-x border-t border-transparent px-2.5 text-xs transition-[width,gap,padding] duration-200 ease-out',
              isCompact ? 'w-[92px]' : 'w-[160px]',
              active
                ? 'border-border bg-background text-foreground'
                : // `bg-overlay-hover`, not a hand-written `bg-accent/40`:
                  // on the new dark ladder `--accent` (17.6%) sits barely
                  // above this strip's own tint, so the old hover moved
                  // the surface by about one lightness point and read as
                  // nothing. The overlay token is a real step.
                  'text-muted-foreground hover:bg-overlay-hover hover:text-foreground',
              // `z-10` is what merges the active tab with the panel: it has
              // to paint over the strip's separator, which the `z-0` hairline
              // child would otherwise draw across its bottom edge.
              active && 'z-10',
            )}
            onClick={() => setActiveTab(d.tab.id)}
          >
            {/* Shoulders first: the carves have to erase the tab's own
                bottom corners (fill *and* side border), so they must paint
                over it but under nothing the user can see. */}
            {active && <TabJoinCorners stripClass="bg-card/40" />}
            <span className={cn('shrink-0', active && d.iconColor)}>{d.icon}</span>
            <span
              // Title is the main element that grows. It always truncates
              // — but the *amount* of room it gets depends on mode:
              //  - normal: ~84px (≈ 5 Chinese chars). The hint (e.g.
              //    "订单生命周期") sits to the right of title.
              //  - compact: ~36px (≈ 2 Chinese chars). Hint hidden.
              // The full title remains available on hover via the
              // `title` attribute.
              //
              // The weight step lives on the label, not on the tab: it is
              // the one element whose type changes, and the method pill and
              // the icons beside it should not go bold with it.
              className={cn(
                'min-w-0 truncate',
                isCompact ? 'max-w-[36px]' : 'max-w-[88px] flex-1',
                active && 'font-semibold',
              )}
              title={d.title}
            >
              {d.title}
            </span>
            {/*
             * The trailing controls travel together as ONE flex item so
             * they can be pushed to the tab's right edge with `ml-auto`.
             *
             * The title carries `flex-1` but is also capped at
             * `max-w-[88px]`, which is what guarantees the hint its room.
             * That cap means a short title leaves free space the flexbox
             * has nowhere to put: it fell at the *end* of the row, after
             * the close button, so `ttt ×` sat in the left third of a
             * 160px tab with a dead gap behind the ×. `ml-auto` moves that
             * free space in front of the group instead, which is where a
             * browser tab puts it — title left, × pinned right.
             *
             * One wrapper rather than `ml-auto` on the close button itself:
             * `Tooltip` wraps its child in a `relative inline-flex` span,
             * so that span is the flex item, not the button inside it.
             */}
            <span className="ml-auto inline-flex shrink-0 items-center gap-1.5">
              {dirty && (
                <Tooltip content="有未保存的修改" side="bottom">
                  <span
                    className="h-1.5 w-1.5 shrink-0 rounded-full bg-warning"
                    aria-label="有未保存的修改"
                  />
                </Tooltip>
              )}
              {d.hint && !isCompact && (
                <Tooltip content={d.hint} side="bottom">
                  <span className="max-w-[40px] truncate text-[10px] text-faint">{d.hint}</span>
                </Tooltip>
              )}
              <Tooltip content="关闭" side="bottom">
                <button
                  type="button"
                  aria-label="关闭标签"
                  // Stop propagation so the close click doesn't also activate.
                  onClick={(e) => {
                    e.stopPropagation();
                    requestCloseTab(d.tab.id);
                  }}
                  className={cn(
                    'flex h-4 w-4 shrink-0 items-center justify-center rounded-sm text-faint hover:bg-overlay-hover hover:text-foreground',
                    // Always show on hover, but keep visible on the active tab.
                    active ? 'opacity-100' : 'opacity-0 group-hover:opacity-100',
                  )}
                >
                  <X className="h-3 w-3" />
                </button>
              </Tooltip>
            </span>
            {/* Active-tab indicator moved to a strip-level element below
                so it can tween between tabs as a single persistent node. */}
          </div>
        );
      })}

      {/* Persistent active-tab indicator. It lives at the strip level so
          it can survive a tab-set change as one element, and its geometry
          is written every frame while layout is in motion — see the
          "Active-tab indicator" block above. `w-0` is deliberately absent
          from the class list: the width is entirely the driver's to
          write, and a class-level width would just be an inline override
          fighting it. */}
      <span
        ref={barRef}
        aria-hidden
        className="pointer-events-none absolute left-0 top-0 z-20 h-0.5 rounded-t-md bg-primary opacity-0"
      />

      <div className="ml-auto flex items-center pr-1">
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 text-faint"
          onClick={() => {
            // "Close all but active" affordance — useful when the user
            // ends up with dozens of tabs from a deep tree dive. Routes
            // through the dirty guard so a mid-rename in another tab
            // gets a chance to resolve.
            requestCloseOthers();
          }}
          aria-label="关闭其他标签"
          title="关闭其他标签"
        >
          <ScrollText className="h-3.5 w-3.5" />
        </Button>
      </div>

      {menu && (
        <TabContextMenu
          anchor={{ x: menu.x, y: menu.y }}
          tabId={menu.tabId}
          openTabs={openTabs}
          dirtyTabIds={dirtyTabIds}
          onClose={() => setMenu(null)}
          onCloseTab={(id) => {
            requestCloseTab(id);
            setMenu(null);
          }}
          onCloseAll={() => {
            // The dirty guard inside requestCloseAllTabs will surface
            // the DirtyCloseDialog if any tab has unsaved changes, so
            // we no longer need the brittle window.confirm() prompt.
            requestCloseAllTabs();
            setMenu(null);
          }}
          onCloseSaved={() => {
            const n = closeSavedTabs();
            if (n === 0) {
              window.alert('没有可关闭的已保存标签（其余都有未保存的修改）。');
            }
            setMenu(null);
          }}
        />
      )}
    </div>
  );
}

// ----------------------------------------------------------------
//  Tab descriptors — built from the raw openTabs + entity lookups.
//  `useTabDescriptors` only depends on the openTabs array reference and
//  the entity arrays, so it re-runs when any of them changes.
// ----------------------------------------------------------------

function useTabDescriptors(tabs: OpenTab[]): TabDescriptor[] {
  const projects = useAppStore((s) => s.projects);
  // One node array for module / collection / endpoint; the detail map
  // only for the endpoint's method pill.
  const nodes = useAppStore((s) => s.nodes);
  const endpointDetails = useAppStore((s) => s.endpointDetails);
  // Unsaved endpoints live only here — never in `nodes`. Without this the
  // strip would render them through the "已删除" fallback, which is the
  // one thing a draft definitively is not.
  const draftEndpoints = useAppStore((s) => s.draftEndpoints);

  // Manual map instead of .map().filter() to keep the shape stable when
  // the entity is missing (fall back to a "已删除" label rather than
  // rendering nothing — pruneStaleTabs will clean it up next tick).
  const out: TabDescriptor[] = [];
  for (const t of tabs) {
    if (t.kind === 'settings') {
      // The settings tab is a singleton — no underlying entity to look
      // up. Title is fixed; section selection lives inside the panel,
      // not on the tab strip, so we don't surface it here.
      out.push({
        tab: t,
        title: '设置',
        icon: <Settings2 className="h-3.5 w-3.5" />,
        iconColor: 'text-muted-foreground',
      });
      continue;
    }
    if (t.kind === 'project') {
      const p = projects.find((x) => x.id === t.id);
      if (!p) {
        out.push(missingDescriptor(t));
        continue;
      }
      out.push({
        tab: t,
        title: p.name,
        hint: p.description || undefined,
        icon: <Package className="h-3.5 w-3.5" />,
        iconColor: HUE.project.text,
      });
      continue;
    }
    if (t.kind === 'module') {
      const m = nodes.find((x) => x.id === t.id);
      if (!m) {
        out.push(missingDescriptor(t));
        continue;
      }
      out.push({
        tab: t,
        title: m.name,
        hint: m.description || undefined,
        icon: m.expanded ? (
          <FolderOpen className="h-3.5 w-3.5" />
        ) : (
          <FolderClosed className="h-3.5 w-3.5" />
        ),
        iconColor: HUE.module.text,
      });
      continue;
    }
    if (t.kind === 'collection') {
      const c = nodes.find((x) => x.id === t.id);
      if (!c) {
        out.push(missingDescriptor(t));
        continue;
      }
      out.push({
        tab: t,
        title: c.name,
        hint: c.description || undefined,
        icon: c.expanded ? (
          <FolderOpen className="h-3.5 w-3.5" />
        ) : (
          <FolderClosed className="h-3.5 w-3.5" />
        ),
        iconColor: HUE.collection.text,
      });
      continue;
    }
    // endpoint-draft — same pill treatment as a saved endpoint, because it
    // is the same object as far as the strip is concerned; the only
    // difference is that its title is still being typed.
    if (t.kind === 'endpoint-draft') {
      const d = draftEndpoints[t.id];
      if (!d) {
        out.push(missingDescriptor(t));
        continue;
      }
      out.push({
        tab: t,
        title: d.name,
        icon: (
          <MethodPill
            method={d.method}
            size="xs"
            variant="solid"
            title={`HTTP ${d.method}`}
            className="!h-3.5 !w-3.5 !text-[9px]"
          />
        ),
        iconColor: 'text-muted-foreground',
      });
      continue;
    }
    // endpoint — the method pill comes from the detail row, not the
    // node, so a tab whose configuration is missing falls back to the
    // "已删除" descriptor rather than rendering an empty pill.
    const e = nodes.find((x) => x.id === t.id);
    const eDetail = e ? endpointDetails[e.id] : undefined;
    if (!e || !eDetail) {
      out.push(missingDescriptor(t));
      continue;
    }
    out.push({
      tab: t,
      title: e.name,
      icon: (
        <MethodPill
          method={eDetail.method}
          size="xs"
          variant="solid"
          title={`HTTP ${eDetail.method}`}
          className="!h-3.5 !w-3.5 !text-[9px]"
        />
      ),
      iconColor: 'text-muted-foreground',
    });
  }
  return out;
}

function missingDescriptor(tab: OpenTab): TabDescriptor {
  return {
    tab,
    // A draft reaching here means its document was discarded while the tab
    // was still open. "已删除" would be wrong — nothing was ever saved —
    // and the label is the only signal the user gets, so it has to name
    // what actually happened.
    title: tab.kind === 'endpoint-draft' ? '（草稿已丢弃）' : `（${KIND_LABEL[tab.kind]}已删除）`,
    icon: <Hash className="h-3.5 w-3.5" />,
    iconColor: 'text-destructive',
  };
}

// ----------------------------------------------------------------
//  Right-click context menu
// ----------------------------------------------------------------

interface TabContextMenuProps {
  anchor: { x: number; y: number };
  tabId: string;
  openTabs: OpenTab[];
  dirtyTabIds: Set<string>;
  onClose: () => void;
  onCloseTab: (id: string) => void;
  onCloseAll: () => void;
  onCloseSaved: () => void;
}

function TabContextMenu({
  anchor,
  tabId,
  openTabs,
  dirtyTabIds,
  onClose,
  onCloseTab,
  onCloseAll,
  onCloseSaved,
}: TabContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);

  // Close on outside click / Esc. We watch mousedown so a click on
  // an item still registers the item's own handler first.
  useEffect(() => {
    const onMouseDown = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('mousedown', onMouseDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onMouseDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [onClose]);

  // Clamp to viewport so the menu never spawns off-screen.
  const style: React.CSSProperties = {
    position: 'fixed',
    top: anchor.y,
    left: anchor.x,
    // First-render guess — the clamp below refines it.
  };
  // After mount, measure and nudge inside the viewport.
  useEffect(() => {
    const el = menuRef.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    if (r.right > window.innerWidth - 4) {
      el.style.left = `${Math.max(4, window.innerWidth - r.width - 4)}px`;
    }
    if (r.bottom > window.innerHeight - 4) {
      el.style.top = `${Math.max(4, window.innerHeight - r.height - 4)}px`;
    }
  }, [anchor.x, anchor.y]);

  const savedCount = openTabs.filter((t) => !dirtyTabIds.has(t.id)).length;
  const targetDirty = dirtyTabIds.has(tabId);

  return (
    <div
      ref={menuRef}
      role="menu"
      style={style}
      className="z-[200] min-w-[200px] overflow-hidden rounded-md border border-border bg-popover py-1 text-xs shadow-lg"
    >
      <MenuItem
        icon={<X className="h-3.5 w-3.5" />}
        label="关闭"
        shortcut={targetDirty ? '有未保存' : undefined}
        onClick={() => onCloseTab(tabId)}
      />
      <MenuItem
        icon={<CheckCircle2 className="h-3.5 w-3.5" />}
        label="关闭已保存"
        shortcut={savedCount === 0 ? '无可关闭' : `${savedCount} 个`}
        disabled={savedCount === 0}
        onClick={onCloseSaved}
      />
      <MenuItem
        icon={<XSquare className="h-3.5 w-3.5" />}
        label="关闭所有"
        shortcut={`${openTabs.length} 个`}
        onClick={onCloseAll}
      />
    </div>
  );
}

function MenuItem({
  icon,
  label,
  shortcut,
  disabled,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  shortcut?: string;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      onClick={onClick}
      className={cn(
        'flex w-full items-center gap-2 px-2.5 py-1.5 text-left transition-colors',
        // Same reasoning as the tab row above: `--accent` is barely above
        // the popover it sits on, so a solid `bg-accent` hover is a
        // no-op on the new ladder.
        'hover:bg-overlay-hover hover:text-foreground',
        'focus:bg-overlay-hover focus:text-foreground focus:outline-none',
        disabled &&
          'cursor-not-allowed text-muted-foreground/50 hover:bg-transparent hover:text-muted-foreground/50',
      )}
    >
      <span className="shrink-0 text-muted-foreground">{icon}</span>
      <span className="flex-1">{label}</span>
      {shortcut && <span className="shrink-0 text-[10px] text-faint">{shortcut}</span>}
    </button>
  );
}
