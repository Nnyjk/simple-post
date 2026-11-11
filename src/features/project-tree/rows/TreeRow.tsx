import {
  useRef,
  type CSSProperties,
  type DragEvent as ReactDragEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  type RefObject,
} from 'react';
import { ChevronRight, Plus } from 'lucide-react';
import { cn, focusRing } from '@/lib/utils';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';
import { Tooltip } from '@/components/ui/tooltip';
import { InlineEdit } from '@/components/ui/InlineEdit';
import type { DragContextValue, TreeNodeKind } from '../tree-types';
import { DropHintOverlay } from './DropHintOverlay';

/**
 * 一行里三块东西的高度：箭头、名称方块、行尾的「新建」按钮。
 *
 * 箭头与名称方块必须**同高** —— 箭头只有一枚图标，名称那一块里是一段文字，两者同高
 * 时图标与文字落在同一条水平线上。高度写死在这儿（26px）而不是由字号推出来：名称
 * 那一块里装的是一段文字，行高会随字体设置变，写死的高度不会变。
 */
const BLOCK_H = 'h-[26px]';

/**
 * 名称方块的边框与阴影：从左向右渐变消失。
 *
 * 做法是给一层「只画边框与阴影」的覆盖层加遮罩 —— 遮罩只作用在那一层上，文字不跟着淡。
 * 这一层平时是透明的：鼠标悬在这一块上时，GSAP 把它淡入，边框与阴影才显出来。
 */
const FADE_BORDER_STYLE: CSSProperties = {
  WebkitMaskImage: 'linear-gradient(to right, black 0%, black 25%, transparent 100%)',
  maskImage: 'linear-gradient(to right, black 0%, black 25%, transparent 100%)',
};

/**
 * 前导槽的宽度：模块行与集合行的箭头占这一格，接口行没有箭头，留一格同样宽的空位。
 *
 * 两行用同一个宽度常数，名称方块的左边界便落在同一个位置 —— 于是**接口行的名称方块与
 * 上级的名称方块左边界对齐**（接口是叶子，没有自己的层级）。
 *
 * 这一格只比箭头图标本身宽一点：箭头那一块只是一枚图标，多出来的宽度会变成箭头与
 * 名称之间的一段空白。
 */
const SLOT_W = 'w-[16px]';

/**
 * 那些块的边框：鼠标悬在哪一块上，哪一块的边框才显出来，离开时收回。
 *
 * 边框与阴影画在一层覆盖上（覆盖层平时透明），故动画只动 `opacity` —— 边框的颜色留在
 * CSS 里，不搬到 JS 里再补间一遍。
 *
 * 悬停的判定挂在覆盖层所在的按钮上：覆盖层自己不吃指针事件，鼠标进出按钮才是这一块
 * 的进出。
 *
 * 减弱动效时不播补间，直接落到终态：边框该显出来就显出来。
 */
function useHoverBorders(borderRefs: RefObject<HTMLSpanElement | null>[]) {
  useGSAP(() => {
    const targets = borderRefs
      .map((ref) => ref.current)
      .filter((el): el is HTMLSpanElement => el !== null);
    if (!targets.length) return;

    // 边框的显隐是状态，不是装饰：监听无条件注册，只决定「怎么改 opacity」。
    // 默认是直落（不做补间）；允许播动画时换成补间版。
    let fade = (border: HTMLSpanElement, opacity: number) => gsap.set(border, { opacity });
    const cleanup = motionSafe(
      () => {
        fade = (border, opacity) => gsap.set(border, { opacity });
      },
      () => {
        fade = (border, opacity) =>
          gsap.to(border, { opacity, duration: 0.15, ease: 'power2.out', overwrite: 'auto' });
      },
    );

    const offs = targets.map((border) => {
      // 覆盖层画在按钮里面，它的父元素就是那一块的按钮 —— 鼠标进出按钮即是进出这一块。
      const host = border.parentElement;
      const enter = () => fade(border, 1);
      const leave = () => fade(border, 0);
      host?.addEventListener('mouseenter', enter);
      host?.addEventListener('mouseleave', leave);
      return () => {
        host?.removeEventListener('mouseenter', enter);
        host?.removeEventListener('mouseleave', leave);
        gsap.killTweensOf(border);
      };
    });

    return () => {
      cleanup();
      offs.forEach((off) => off());
    };
  });
}

export interface TreeRowProps {
  /**
   * 这一行的类型。它决定这一行在拖放里扮演什么角色 —— 模块、集合与接口的落点规则
   * 各不相同，那些规则住在 `../drop-resolve.ts`，不在这一层。
   */
  kind: TreeNodeKind;
  /** 行 id。 */
  id: string;
  /** 行上显示的名字。 */
  name: string;
  /** 这一行是否正在被重命名。 */
  isEditing: boolean;
  /**
   * 展开 / 收起的箭头。给了这一项，行左侧就多一个**独立的小按钮**。
   *
   * 两件事刻意分开：行按钮只打开工作区，箭头只切换展开状态。
   */
  toggle?: {
    /** 这一行当前是否展开。 */
    expanded: boolean;
    /** 点箭头时调用。 */
    onToggle: () => void;
  };
  /**
   * 前导内容：接口行的 method 药丸。
   *
   * 药丸留在名称方块**里面** —— 它与名称是一体的（药丸在前，名称在后）。药丸若排在
   * 名称方块外面，接口行的名称便会落到上级名称的右边。
   */
  leading?: ReactNode;
  /** 按钮内的后置内容：接口行的「已选」徽标。 */
  badge?: ReactNode;
  /** 行尾的「新建」按钮组。模块与集合有，接口没有。 */
  onPlusClick?: (e: ReactMouseEvent) => void;
  onSelect: (e: ReactMouseEvent) => void;
  onStartEdit: () => void;
  onSaveEdit: (name: string) => void;
  onCancelEdit: () => void;
  onContextMenu: (e: ReactMouseEvent) => void;
  /** 拖放接线。行自己按 `kind` 与 `id` 接上回调，落点提示也自己从里面读。 */
  dragContext: DragContextValue;
  /** 被拖走的 id 集合。默认只有这一行自己。 */
  dragIds?: string[];
  /** 行容器的额外 class。 */
  outerClassName?: string;
  /** 行按钮的额外 class。 */
  buttonClassName?: string;
  /** 这一行是否带 `data-tree-row` 标记 —— 展开动画按它逐行淡入。 */
  dataRow?: boolean;
}

/**
 * 树里三种行的共同外壳：行容器、行按钮、拖放回调、落点提示与行内改名。
 *
 * 三种行原本各写一遍这整套接线。改动落在三处时（三次「重命名时那一行退出拖拽」就是
 * 这么来的），三处都得改，而漏掉一处不会有任何报错。故接线收进这一层，三种行各自只
 * 留下真正不同的部分：前置图标、尾部徽标、行尾的「新建」按钮，以及各自的 class。
 *
 * 一行由三块组成：前导槽（箭头方块或 method 药丸）、名称方块、行尾的「新建」按钮。
 * 展开箭头是**另一个**按钮，与名称方块平级：它只切换展开状态，不打开工作区。
 */
export function TreeRow({
  kind,
  id,
  name,
  isEditing,
  toggle,
  leading,
  badge,
  onPlusClick,
  onSelect,
  onStartEdit,
  onSaveEdit,
  onCancelEdit,
  onContextMenu,
  dragContext,
  dragIds,
  outerClassName,
  buttonClassName,
  dataRow,
}: TreeRowProps) {
  const dropHint =
    dragContext.dropHint && dragContext.dropHint.key === `${kind}:${id}`
      ? dragContext.dropHint.zone
      : null;

  /** 两块各有一层默认透明的边框覆盖层：悬浮时由 GSAP 淡入。 */
  const arrowBorderRef = useRef<HTMLSpanElement>(null);
  const nameBorderRef = useRef<HTMLSpanElement>(null);
  useHoverBorders([arrowBorderRef, nameBorderRef]);

  /** 行按钮是这一行的拖拽起点 —— 箭头不参与。 */
  const startDrag = (e: ReactDragEvent) => {
    dragContext.setDataTransfer(e, { kind, ids: dragIds ?? [id] });
  };

  return (
    <div
      data-tree-row={dataRow ? '' : undefined}
      onContextMenu={onContextMenu}
      className={cn('group mb-0.5 rounded-md', outerClassName)}
    >
      <div
        className={cn(
          'relative flex items-center gap-0.5 rounded-sm transition-colors',
          dropHint === 'into' && 'bg-primary/15 ring-1 ring-primary/40',
        )}
      >
        {toggle && (
          <button
            type="button"
            // 箭头不发起拖拽：它只切换展开状态，拖它没有意义。这个按钮保留
            // onDragOver 与 onDrop —— 把别的节点拖到箭头那一小条上，与拖到名称上
            // 是同一件事。
            onDragOver={dragContext.makeDragOverHandler({ kind, id })}
            onDragLeave={dragContext.makeDragLeaveHandler({ kind, id })}
            onDrop={dragContext.makeDropHandler({ kind, id })}
            onClick={toggle.onToggle}
            aria-expanded={toggle.expanded}
            aria-label={toggle.expanded ? '收起' : '展开'}
            title={toggle.expanded ? '收起' : '展开'}
            data-testid="tree-toggle-btn"
            className={cn(
              'relative flex shrink-0 items-center justify-center rounded-md',
              BLOCK_H,
              SLOT_W,
              'text-faint transition-colors hover:bg-overlay-hover hover:text-foreground',
              focusRing,
            )}
          >
            {/* 边框与阴影由这一层画，它不吃指针事件；平时透明，悬停时由 GSAP 淡入。 */}
            <span
              ref={arrowBorderRef}
              aria-hidden
              className="pointer-events-none absolute inset-0 rounded-md border border-border shadow-sm opacity-0"
            />
            <ChevronRight
              className={cn(
                'h-3 w-3 transition-transform',
                toggle.expanded && 'rotate-90 text-muted-foreground',
              )}
            />
          </button>
        )}
        {!toggle && leading && (
          // 接口行没有箭头，但它的名称方块要与**上级的名称方块**左边界对齐 —— 左侧
          // 留出与箭头同样宽的一格（间隙由行容器的 gap 给出），名称方块便落在上级
          // 名称方块的那一条竖线上。
          <span aria-hidden className={cn('shrink-0', SLOT_W)} />
        )}
        <button
          type="button"
          // 正在改名的那一行不参与拖拽。理由见 `EndpointRow.tsx` 的同一处：重命名输入框就在
          // 这个 button 里面，而 `draggable` 的祖先会把「在输入框里框选文字」的手势读成
          // 「拖这一行」。
          draggable={!isEditing}
          onDragStart={startDrag}
          onDragEnd={dragContext.handleDragEnd}
          onDragOver={dragContext.makeDragOverHandler({ kind, id })}
          onDragLeave={dragContext.makeDragLeaveHandler({ kind, id })}
          onDrop={dragContext.makeDropHandler({ kind, id })}
          onClick={onSelect}
          onDoubleClick={(e) => {
            if (!isEditing) {
              e.preventDefault();
              e.stopPropagation();
              onStartEdit();
            }
          }}
          className={cn(
            'relative flex min-w-0 flex-1 items-center gap-1.5 rounded-md px-1.5',
            'text-left text-xs text-foreground transition-colors',
            BLOCK_H,
            buttonClassName,
          )}
        >
          {/* 边框与阴影由这一层画，它不吃指针事件；平时透明，悬停时由 GSAP 淡入。 */}
          <span
            ref={nameBorderRef}
            aria-hidden
            style={FADE_BORDER_STYLE}
            className="pointer-events-none absolute inset-0 rounded-md border border-border shadow-sm opacity-0"
          />
          {leading}
          <InlineEdit
            value={name}
            onSave={onSaveEdit}
            // 由树的 `editing` 目标受控，而非本地的双击：从「+」菜单创建的行必须能把新行
            // 直接丢进重命名，而那个信号是树状态传来的。本行只响应 `false`（用户完成或取消）
            // 这一个跃迁 —— 打开是树的事，行自己的双击通过上面的 `onStartEdit` 抵达它。
            editing={isEditing}
            onEditingChange={(open) => {
              if (!open) onCancelEdit();
            }}
            className="flex-1"
            display={() => <span className="flex-1 truncate text-left">{name}</span>}
          />
          {badge}
        </button>
        {onPlusClick && (
          <div className="invisible flex shrink-0 items-center pr-1 group-hover:visible">
            <Tooltip content="新建" side="top">
              <button
                type="button"
                onClick={onPlusClick}
                onDoubleClick={(e) => e.stopPropagation()}
                aria-label="新建"
                className="flex h-5 w-5 items-center justify-center rounded-sm text-muted-foreground hover:bg-accent hover:text-foreground"
              >
                <Plus className="h-3 w-3" />
              </button>
            </Tooltip>
          </div>
        )}
        <DropHintOverlay zone={dropHint} />
      </div>
    </div>
  );
}
