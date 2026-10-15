import { useEffect, useRef } from 'react';
import { X, CheckCircle2, XSquare } from 'lucide-react';
import type { OpenTab } from '@/stores/app-store';
import { cn } from '@/lib/utils';
export interface TabContextMenuProps {
  anchor: { x: number; y: number };
  tabId: string;
  openTabs: OpenTab[];
  dirtyTabIds: Set<string>;
  onClose: () => void;
  onCloseTab: (id: string) => void;
  onCloseAll: () => void;
  onCloseSaved: () => void;
}

export function TabContextMenu({
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

  // 外部点击 / Esc 关闭。监听 mousedown，好让点击菜单项时先跑它自己的处理器。
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

  // 夹到视口内，菜单不会生成在屏幕外。
  const style: React.CSSProperties = {
    position: 'fixed',
    top: anchor.y,
    left: anchor.x,
    // 首帧的猜测值 —— 下面的夹取会修正它。
  };
  // 挂载后测量并微调回视口内。
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
        // 同上面标签行的理由：`--accent` 与它所处的 popover 底色几乎相同，
        // 实心 `bg-accent` 的 hover 在新阶梯上是空操作。
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
