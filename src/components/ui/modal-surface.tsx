/**
 * 模态层原语 —— 承载「挡住背景、只与最上层对话」的那一层。
 *
 * 仓库里所有全屏模态都走它：`Dialog`（对话框）与 `CommandPalette`（命令面板）。
 * 它负责的事只有这些，且每一件都只有一处实现：
 *
 * - portal 到 `document.body`，好逃出树里任何 `overflow` 父级。
 * - Esc 关闭；点遮罩关闭（`disableBackdropClose` 可关掉后者）。
 * - 打开时锁住 body 滚动。
 * - 焦点陷阱：Tab 在面板里循环，关闭后焦点回到打开它的那个元素。
 * - 背景置为 `inert` 且对读屏隐藏：背景既点不到、Tab 不进去、也不被读出来。
 * - 统一的入场动画与统一的层级。
 *
 * 面板长什么样由调用方决定：`Dialog` 给它加了标题栏与页脚，命令面板给了它搜索行与
 * 快捷键提示条。刻意不在这里规定形状 —— 两者的差别是形状的差别，不是模态语义的差别，
 * 把形状也塞进来只会让原语长出一排互相牵制的开关。
 */

import { useEffect, useRef, type MouseEvent, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { cn } from '@/lib/utils';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';
import { LAYER } from './layers';

/**
 * 面板的可访问名。二选一：字符串直接当 `aria-label`，元素 id 走 `aria-labelledby`。
 * 写成联合类型是为了逼调用方明确选一种 —— 两者都不给的面板，读屏软件报不出它是什么。
 */
export type ModalLabel =
  { ariaLabel: string; ariaLabelledBy?: never } | { ariaLabel?: never; ariaLabelledBy: string };

interface ModalSurfaceProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 面板内容。 */
  children: ReactNode;
  /** 面板的可访问名。 */
  label: ModalLabel;
  /** 面板在视口里的纵向位置。 */
  align?: 'center' | 'top';
  /** 面板宽度。 */
  size?: 'sm' | 'md' | 'lg';
  /** 面板的类名。用来改配色、圆角、阴影。 */
  className?: string;
  /** 点遮罩不关闭。用于必须让用户显式选一个动作的阻塞式确认框。 */
  disableBackdropClose?: boolean;
  /** 描述文本的 id。给了之后读屏会连正文一起念。 */
  ariaDescribedBy?: string;
}

const SIZE_CLASS: Record<NonNullable<ModalSurfaceProps['size']>, string> = {
  sm: 'max-w-sm',
  md: 'max-w-md',
  lg: 'max-w-xl',
};

const ALIGN_CLASS: Record<NonNullable<ModalSurfaceProps['align']>, string> = {
  center: 'items-center justify-center p-4',
  top: 'items-start justify-center px-4 pt-[12vh]',
};

/** 面板内可以聚焦的元素。 */
const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** 面板内当前真正可聚焦的元素。隐藏的（`display:none`）不算。 */
function focusableIn(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) => el.getClientRects().length > 0,
  );
}

export function ModalSurface({
  open,
  onOpenChange,
  children,
  label,
  align = 'center',
  size = 'md',
  className,
  disableBackdropClose,
  ariaDescribedBy,
}: ModalSurfaceProps) {
  const overlayRef = useRef<HTMLDivElement>(null);
  const backdropRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  // Esc 关闭与 Tab 循环，共用一个捕获阶段的监听器。
  //
  // 捕获阶段是必须的：React 的事件系统挂在 root 容器上，冒泡阶段才轮到它。
  // Esc 在这里 `stopPropagation()` 之后，React 的 `onKeyDown` 就再也收不到它 ——
  // 于是「按 Esc 关掉」这件事全仓库只有这一处实现，不会出现两个处理器各关一次。
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onOpenChange(false);
        return;
      }
      if (e.key !== 'Tab') return;
      const panel = panelRef.current;
      if (!panel) return;
      const items = focusableIn(panel);
      if (items.length === 0) {
        e.preventDefault();
        panel.focus();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement as HTMLElement | null;
      const inside = active !== null && panel.contains(active);
      if (e.shiftKey) {
        if (!inside || active === first) {
          e.preventDefault();
          last.focus();
        }
      } else if (!inside || active === last) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener('keydown', onKeyDown, true);
    return () => document.removeEventListener('keydown', onKeyDown, true);
  }, [open, onOpenChange]);

  // 背景置为 inert，关闭时归还焦点。
  //
  // 归还在解除 inert 之后做：inert 里的元素不接受焦点，先 `focus()` 会被浏览器丢掉。
  useEffect(() => {
    if (!open) return;
    const overlay = overlayRef.current;
    if (!overlay) return;
    const previous = document.activeElement as HTMLElement | null;

    const marked: { el: HTMLElement; inert: boolean; ariaHidden: string | null }[] = [];
    for (const child of Array.from(document.body.children)) {
      if (!(child instanceof HTMLElement)) continue;
      // 本层与别的模态层都不标记 —— 它们同样是可交互的界面。
      if (child.contains(overlay)) continue;
      if (child.querySelector('[data-modal-surface]')) continue;
      marked.push({
        el: child,
        inert: child.hasAttribute('inert'),
        ariaHidden: child.getAttribute('aria-hidden'),
      });
      child.setAttribute('inert', '');
      child.setAttribute('aria-hidden', 'true');
    }

    return () => {
      for (const m of marked) {
        if (!m.inert) m.el.removeAttribute('inert');
        if (m.ariaHidden === null) m.el.removeAttribute('aria-hidden');
        else m.el.setAttribute('aria-hidden', m.ariaHidden);
      }
      if (previous && previous !== document.body && document.contains(previous)) {
        previous.focus();
      }
    };
  }, [open]);

  // 打开时把焦点移进面板。
  //
  // 面板里若有 `autoFocus` 的元素，浏览器在挂载那一刻已经聚焦了它，此时不再抢。
  // 延后一拍是必要的：挂载之前输入框还不存在。
  useEffect(() => {
    if (!open) return;
    const timer = window.setTimeout(() => {
      const panel = panelRef.current;
      if (!panel) return;
      if (panel.contains(document.activeElement)) return;
      const first = focusableIn(panel)[0];
      (first ?? panel).focus();
    }, 0);
    return () => window.clearTimeout(timer);
  }, [open]);

  // 打开时锁住 body 滚动。
  useEffect(() => {
    if (!open) return;
    const prevOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      document.body.style.overflow = prevOverflow;
    };
  }, [open]);

  // 入场动画 —— 每次面板挂载（open 翻成 true）都跑。没有出场动画：open=false 会整体
  // 卸载 portal，这样就够。
  //
  // 减弱动效下面板直接出现。它照样挂载、照样困住焦点、照样有背景，所以它的行为没有
  // 任何一点依赖这段过渡跑起来。
  useGSAP(
    () => {
      return motionSafe(() => {
        if (backdropRef.current) {
          gsap.fromTo(
            backdropRef.current,
            { opacity: 0 },
            { opacity: 1, duration: 0.18, ease: 'none' },
          );
        }
        if (panelRef.current) {
          gsap.fromTo(
            panelRef.current,
            { opacity: 0, scale: 0.97 },
            { opacity: 1, scale: 1, duration: 0.22, ease: 'power2.out' },
          );
        }
      });
    },
    { scope: overlayRef, dependencies: [open] },
  );

  const handleOverlayMouseDown = (e: MouseEvent<HTMLDivElement>) => {
    if (disableBackdropClose) return;
    const panel = panelRef.current;
    if (panel && e.target instanceof Node && panel.contains(e.target)) return;
    onOpenChange(false);
  };

  if (!open || typeof document === 'undefined') return null;

  return createPortal(
    <div
      ref={overlayRef}
      data-modal-surface=""
      className={cn('fixed inset-0 flex', LAYER.modal, ALIGN_CLASS[align])}
      onMouseDown={handleOverlayMouseDown}
    >
      <div
        ref={backdropRef}
        className="absolute inset-0 bg-black/50 backdrop-blur-sm"
        aria-hidden="true"
      />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={label.ariaLabel}
        aria-labelledby={label.ariaLabelledBy}
        aria-describedby={ariaDescribedBy}
        tabIndex={-1}
        className={cn(
          'relative z-10 w-full rounded-lg border border-border shadow-2xl',
          SIZE_CLASS[size],
          className,
        )}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}
