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
 *   - Visual state: active tab uses the primary tint, others stay muted.
 *   - Tab positions slide rather than snap when the tab set changes
 *     (add / remove / reorder). This is implemented as a FLIP animation
 *     via GSAP Flip — see the useGSAP block below.
 *   - The 0.5px primary stripe under the active tab is a single
 *     persistent element at the strip level; its x/width are tweened to
 *     match the active tab on every render so the indicator smoothly
 *     follows active-tab switches.
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
  settings: '设置',
};

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
  // Persistent indicator bar (the 0.5px primary stripe under the active
  // tab). It lives at the strip level instead of inside each tab so that
  // we can tween it as a single element whenever the active tab changes.
  const barRef = useRef<HTMLSpanElement>(null);
  // First-run guard for the bar: snap into place on initial mount rather
  // than running an entry tween from (0,0) to the first active tab.
  const isFirstBarRun = useRef(true);

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

  // Active-bar tween: track whichever tab is currently active. Re-runs
  // when either the active tab changes *or* the tab set changes (a tab
  // inserted before the active one shifts its target offsetLeft).
  useGSAP(
    () => {
      if (!stripRef.current || !barRef.current || !activeTabId) return;
      const activeEl = stripRef.current.querySelector<HTMLElement>(
        `[data-tab-id="${activeTabId}"]`,
      );
      if (!activeEl) return;
      const x = activeEl.offsetLeft;
      const width = activeEl.offsetWidth;
      if (isFirstBarRun.current) {
        // Snap on the very first paint of the strip.
        gsap.set(barRef.current, { x, width });
        isFirstBarRun.current = false;
        return;
      }
      gsap.to(barRef.current, {
        x,
        width,
        duration: 0.28,
        ease: 'power2.out',
      });
    },
    { dependencies: [activeTabId, openTabs] },
  );

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
        'relative flex h-9 shrink-0 items-stretch gap-0.5 overflow-hidden border-b border-border bg-card/40 pl-0 pr-1.5',
        // Belt-and-braces: hide both the standard scrollbar and the
        // webkit one. The strip itself is `overflow-hidden` so a scrollbar
        // wouldn't actually appear here — but keeping the rules in case a
        // future change ever reintroduces `overflow-x-auto`.
        'scrollbar-none',
      )}
    >
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
                : 'text-muted-foreground hover:bg-accent/40 hover:text-foreground',
            )}
            onClick={() => setActiveTab(d.tab.id)}
          >
            <span className={cn('shrink-0', active && d.iconColor)}>{d.icon}</span>
            <span
              // Title is the main element that grows. It always truncates
              // — but the *amount* of room it gets depends on mode:
              //  - normal: ~84px (≈ 5 Chinese chars). The hint (e.g.
              //    "订单生命周期") sits to the right of title.
              //  - compact: ~36px (≈ 2 Chinese chars). Hint hidden.
              // The full title remains available on hover via the
              // `title` attribute.
              className={cn('min-w-0 truncate', isCompact ? 'max-w-[36px]' : 'max-w-[88px] flex-1')}
              title={d.title}
            >
              {d.title}
            </span>
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
                <span className="max-w-[40px] truncate text-[10px] text-muted-foreground/70">
                  {d.hint}
                </span>
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
                  'flex h-4 w-4 shrink-0 items-center justify-center rounded-sm text-muted-foreground/70 hover:bg-accent hover:text-foreground',
                  // Always show on hover, but keep visible on the active tab.
                  active ? 'opacity-100' : 'opacity-0 group-hover:opacity-100',
                )}
              >
                <X className="h-3 w-3" />
              </button>
            </Tooltip>
            {/* Active-tab indicator moved to a strip-level element below
                so it can tween between tabs as a single persistent node. */}
          </div>
        );
      })}

      {/* Persistent active-tab indicator. The bar's x/width are tweened
          by the useGSAP block above to follow whichever tab is currently
          active — see comment near `barRef`. */}
      <span
        ref={barRef}
        aria-hidden
        className="pointer-events-none absolute left-0 top-0 z-10 h-0.5 w-0 rounded-t-md bg-primary"
      />

      <div className="ml-auto flex items-center pr-1">
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 text-muted-foreground/70"
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
    title: `（${KIND_LABEL[tab.kind]}已删除）`,
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
        'hover:bg-accent hover:text-foreground',
        'focus:bg-accent focus:text-foreground focus:outline-none',
        disabled &&
          'cursor-not-allowed text-muted-foreground/50 hover:bg-transparent hover:text-muted-foreground/50',
      )}
    >
      <span className="shrink-0 text-muted-foreground">{icon}</span>
      <span className="flex-1">{label}</span>
      {shortcut && (
        <span className="shrink-0 text-[10px] text-muted-foreground/60">{shortcut}</span>
      )}
    </button>
  );
}
