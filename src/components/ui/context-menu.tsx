import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import { cn } from '@/lib/utils';
import { LAYER } from '@/components/ui/layers';

export type ContextMenuItem =
  | {
      kind: 'item';
      label: string;
      icon?: ReactNode;
      onClick: () => void;
      /** 高亮为红色（如删除）。 */
      danger?: boolean;
      /** 渲染为不可交互（灰显）。 */
      disabled?: boolean;
    }
  | { kind: 'divider' };

export interface ContextMenuProps {
  open: boolean;
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
}

/** 可聚焦的菜单项。禁用的项不参与方向键循环，故选择器里就排除掉。 */
const FOCUSABLE = '[role="menuitem"]:not([disabled])';

/**
 * 轻量右键菜单，portal 挂到 `document.body`。
 *
 * - 自动翻转以留在视口内；菜单高于视口时自己滚动，不留不可达的尾巴。
 * - 打开即聚焦第一项，↑↓ / Home / End 在启用的项之间循环。
 * - 点击外部、Escape 或选中任意项时关闭；关闭后把焦点还给打开前的那个元素。
 * - 菜单根上屏蔽浏览器原生右键菜单。
 */
export function ContextMenu({ open, x, y, items, onClose }: ContextMenuProps): ReactNode | null {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });
  /** 打开菜单前焦点所在的那个元素，关闭后还给它。 */
  const opener = useRef<HTMLElement | null>(null);

  // 挂载后调整位置，使菜单留在视口内。
  useLayoutEffect(() => {
    if (!open) return;
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    // 菜单自己带 `max-h`，量出来的高度已经是被裁过的；再取一次视口上限是防样式表缺席时
    // 把最后几项放到屏幕外面去。
    const height = Math.min(rect.height, vh - 16);
    let left = x;
    let top = y;
    if (left + rect.width > vw - 8) left = vw - rect.width - 8;
    // 左边缘也要夹住：键盘打开的菜单坐标来自行的矩形，横向滚动之后它可以是负的。
    left = Math.max(8, left);
    if (top + height > vh - 8) top = Math.max(8, vh - height - 8);
    // 指针位置本身就在视口内，这条只兜住「行被滚到视口上方」那种极端情况。
    top = Math.max(8, top);
    setPos({ left, top });
  }, [open, x, y, items.length]);

  // 打开时记住焦点落点并聚焦第一项。
  useEffect(() => {
    if (!open) return;
    opener.current = document.activeElement as HTMLElement | null;
    const first = ref.current?.querySelector<HTMLElement>(FOCUSABLE);
    // `preventScroll`：菜单是 `fixed` 的，聚焦不该把下面的内容滚动一下。
    first?.focus({ preventScroll: true });
    return () => {
      const target = opener.current;
      // 关掉之后再把焦点还回去：此刻菜单项已经卸载，直接在这一帧里 focus 会落空。
      window.setTimeout(() => {
        if (!target?.isConnected) return;
        // 焦点已经落到别的控件上（点了另一个按钮、Tab 走出菜单、树被切走）：那不叫
        // 「关掉菜单」，是用户在别处做事，这时抢回焦点会把下一次按键送到看不见的行上。
        const active = document.activeElement;
        if (active && active !== document.body && active !== target) return;
        target.focus({ preventScroll: true });
      }, 0);
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onClose();
      }
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey, true);
    };
  }, [open, onClose]);

  if (!open || typeof document === 'undefined') return null;

  const onMenuKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onClose();
      return;
    }
    if (e.key === 'Tab') {
      // 菜单项不是 Tab 停点：Tab 关掉菜单，焦点先交还打开前的位置，浏览器随后的默认
      // 动作再从那里往前后走一格。不 preventDefault，不然 Tab 就再也离不开菜单。
      onClose();
      opener.current?.focus({ preventScroll: true });
      return;
    }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp' && e.key !== 'Home' && e.key !== 'End') return;
    const nodes = Array.from(ref.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? []);
    if (nodes.length === 0) return;
    e.preventDefault();
    // 焦点可能已经在菜单外（用户点了空白处但菜单还没关），此时从列表一头起步。
    const at = nodes.indexOf(document.activeElement as HTMLElement);
    const last = nodes.length - 1;
    let next: number;
    if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = last;
    else if (e.key === 'ArrowDown') next = at < 0 || at === last ? 0 : at + 1;
    else next = at <= 0 ? last : at - 1;
    // 视图矮、菜单自己带滚动时，`preventScroll` 只挪焦点不挪滚动位置，End 会把焦点
    // 送到看不见的那一项上，下一次 Enter 就执行了看不见的命令。故显式滚进可视区。
    nodes[next].focus({ preventScroll: true });
    nodes[next].scrollIntoView({ block: 'nearest' });
  };

  return createPortal(
    <div
      ref={ref}
      role="menu"
      style={{ left: pos.left, top: pos.top, position: 'fixed' }}
      className={cn(
        'min-w-[180px] max-h-[calc(100vh-1rem)] overflow-y-auto overscroll-contain rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-lg animate-fade-in',
        LAYER.contextMenu,
      )}
      onContextMenu={(e) => e.preventDefault()}
      onKeyDown={onMenuKeyDown}
    >
      {items.map((it, idx) =>
        it.kind === 'divider' ? (
          <div key={`d-${idx}`} className="my-1 h-px bg-border" role="separator" />
        ) : (
          <button
            key={`i-${idx}`}
            role="menuitem"
            type="button"
            disabled={it.disabled}
            onClick={() => {
              if (it.disabled) return;
              it.onClick();
              onClose();
            }}
            className={cn(
              'flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs transition-colors',
              it.disabled
                ? 'cursor-not-allowed text-faint'
                : it.danger
                  ? 'text-destructive hover:bg-destructive/10'
                  : 'text-foreground hover:bg-overlay-hover',
            )}
          >
            {it.icon && (
              <span className="flex h-3.5 w-3.5 shrink-0 items-center justify-center">
                {it.icon}
              </span>
            )}
            <span className="flex-1 truncate">{it.label}</span>
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}
