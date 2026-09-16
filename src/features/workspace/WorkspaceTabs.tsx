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
 *   - The strip is horizontally scrollable (with the wheel) when there are
 *     more tabs than fit in the available width — common in real projects.
 *   - Keyboard: middle-click closes, just like a browser tab.
 *   - Visual state: active tab uses the primary tint, others stay muted.
 */

import { useEffect, useRef, useState } from 'react';
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
import { cn, methodColorVar } from '@/lib/utils';

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
  // Per-kind accent used on the leading icon, so the strip stays scannable.
  iconColor: string;
}

export function WorkspaceTabs() {
  const openTabs = useAppStore((s) => s.openTabs);
  const activeTabId = useAppStore((s) => s.activeTabId);
  const dirtyTabIds = useAppStore((s) => s.dirtyTabIds);
  const setActiveTab = useAppStore((s) => s.setActiveTab);
  const closeTab = useAppStore((s) => s.closeTab);
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
  useEffect(() => {
    pruneStaleTabs();
  }, [pruneStaleTabs]);

  // Build a descriptor for every tab. Done locally with useMemo so each
  // tab only re-renders when its underlying entity changes.
  const descriptors = useTabDescriptors(openTabs);

  // Keep the active tab in view when the user adds many tabs.
  const stripRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!activeTabId) return;
    const el = stripRef.current?.querySelector<HTMLElement>(
      `[data-tab-id="${activeTabId}"]`,
    );
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
      className="relative flex h-9 shrink-0 items-stretch gap-0.5 overflow-x-auto border-b border-border bg-card/40 pl-0 pr-1.5"
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
              'group relative flex h-full min-w-[120px] max-w-[220px] cursor-pointer items-center gap-1.5 rounded-t-md border-x border-t border-transparent px-2.5 text-xs transition-colors',
              active
                ? 'border-border bg-background text-foreground'
                : 'text-muted-foreground hover:bg-accent/40 hover:text-foreground',
            )}
            onClick={() => setActiveTab(d.tab.id)}
          >
            <span
              className="shrink-0"
              style={{ color: active ? d.iconColor : undefined }}
            >
              {d.icon}
            </span>
            <span className="min-w-0 flex-1 truncate" title={d.title}>
              {d.title}
            </span>
            {dirty && (
              <Tooltip content="有未保存的修改" side="bottom">
                <span
                  className="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-400"
                  aria-label="有未保存的修改"
                />
              </Tooltip>
            )}
            {d.hint && (
              <Tooltip content={d.hint} side="bottom">
                <span className="shrink-0 text-[10px] text-muted-foreground/70">
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
            {active && (
              <span className="pointer-events-none absolute inset-x-0 top-0 h-0.5 rounded-t-md bg-primary" />
            )}
          </div>
        );
      })}

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
//  the four entity arrays, so it re-runs when any of them changes.
// ----------------------------------------------------------------

function useTabDescriptors(tabs: OpenTab[]): TabDescriptor[] {
  const projects = useAppStore((s) => s.projects);
  const modules = useAppStore((s) => s.modules);
  const collections = useAppStore((s) => s.collections);
  const endpoints = useAppStore((s) => s.endpoints);

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
        iconColor: '#a3a3a3',
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
        iconColor: '#a78bfa',
      });
      continue;
    }
    if (t.kind === 'module') {
      const m = modules.find((x) => x.id === t.id);
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
        iconColor: '#fbbf24',
      });
      continue;
    }
    if (t.kind === 'collection') {
      const c = collections.find((x) => x.id === t.id);
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
        iconColor: '#60a5fa',
      });
      continue;
    }
    // endpoint
    const e = endpoints.find((x) => x.id === t.id);
    if (!e) {
      out.push(missingDescriptor(t));
      continue;
    }
    out.push({
      tab: t,
      title: e.name,
      icon: (
        <span
          className="method-letter !h-3.5 !w-3.5 !text-[9px]"
          title={e.method}
          style={{ backgroundColor: methodColorVar(e.method) }}
        >
          {e.method.charAt(0)}
        </span>
      ),
      iconColor: '#a3a3a3',
    });
  }
  return out;
}

function missingDescriptor(tab: OpenTab): TabDescriptor {
  return {
    tab,
    title: `（${KIND_LABEL[tab.kind]}已删除）`,
    icon: <Hash className="h-3.5 w-3.5" />,
    iconColor: '#f87171',
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
        disabled && 'cursor-not-allowed text-muted-foreground/50 hover:bg-transparent hover:text-muted-foreground/50',
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
