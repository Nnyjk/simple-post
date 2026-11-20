import { useRef, type ReactNode } from 'react';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';
import { REVEAL_MAX_ROWS, isBulkExpandFresh, revealPlan } from '../tree-motion';

/** 容器展开的时长：`height` 与 `opacity` 一起走完。 */
const EXPAND_DURATION = 0.25;
/** 容器收起的时长：收起比展开快，手指离开时不必等。 */
const COLLAPSE_DURATION = 0.2;
/** 逐行淡入的时长与位移。 */
const REVEAL_DURATION = 0.2;
const REVEAL_Y = 4;

/**
 * 本容器**自己的**直接行，最多 `limit` 个。
 *
 * 不下潜进嵌套的 `TreeChildren`：那里面的行由各自的容器负责，再淡入一次就是重复动画。
 * 行数只决定「一次浮现多少行」，与数据量无关 —— 200 个接口的集合也只取前几个。
 */
function directRows(el: HTMLElement, limit: number): HTMLElement[] {
  const rows: HTMLElement[] = [];
  const visit = (parent: Element) => {
    for (const child of Array.from(parent.children)) {
      if (rows.length >= limit) return;
      if (child.hasAttribute('data-tree-children')) continue;
      if (child.hasAttribute('data-tree-row')) {
        rows.push(child as HTMLElement);
        continue;
      }
      visit(child);
    }
  };
  visit(el);
  return rows;
}

/**
 * 包裹树节点的子级（模块 → CollectionBranch，或 CollectionRow → 接口 + 子集合），使展开 /
 * 折叠能用 GSAP 动画而非硬挂载 / 卸载。子级始终留在 DOM 中；可见性由容器的 `height` +
 * `opacity` 驱动。展开时另取前几行做一次总时长封顶的错峰淡入，让新内容逐行浮现。
 */
export function TreeChildren({ expanded, children }: { expanded: boolean; children: ReactNode }) {
  const containerRef = useRef<HTMLDivElement>(null);
  // `useGSAP` 除每次依赖变化外，挂载时也会跑一次回调。不加守卫的话，初始即展开的分支会
  // 在首帧从 height: 0 补间到 'auto'，闪一下折叠态。
  const isFirstRun = useRef(true);
  // 上一次注册的偏好监听清理函数。`useGSAP` 在依赖变化时只在卸载才跑上一轮的清理
  // （没开 `revertOnUpdate`），故由本组件自己撤。
  const stopMotion = useRef<(() => void) | null>(null);

  useGSAP(
    () => {
      const el = containerRef.current;
      if (!el) return;

      // 批量动作那条分支直接 return，没有机会交回清理函数：不在这里撤，上一次注册的
      // 监听会留到卸载，偏好翻转时它会把已经收起的容器重新展开。
      stopMotion.current?.();
      stopMotion.current = null;

      // 落到终态：`height` 是 GSAP 拥有的内联样式，跳过补间时也必须写一个确定的值，
      // 否则减弱动效下的展开会停在 `height: 0`。
      //
      // 先杀掉本容器在跑的补间：`gsap.set` 不取消它，收起补间跑到一半时点「全部展开」，
      // 那支补间会在下一帧继续写 `height: 0`，把这里的终态覆盖回去。
      const settle = (open: boolean) => {
        gsap.killTweensOf(el);
        gsap.set(el, { height: open ? 'auto' : 0, opacity: open ? 1 : 0, willChange: 'auto' });
      };

      if (isFirstRun.current) {
        isFirstRun.current = false;
        settle(expanded);
        return;
      }
      // 「全部展开 / 全部收起」让整棵树的展开状态在同一个依赖变化里一起变：容器各自补间
      // 自己的 `height` 时，一帧里就是几百个补间、每帧重新布局整棵子树。窗口内直接落终态。
      if (isBulkExpandFresh()) {
        settle(expanded);
        return;
      }

      if (!expanded) {
        stopMotion.current = motionSafe(
          () => {
            // `overwrite: 'auto'`：快速连点时上一次补间可能还在跑，两条抢同一个 height 会抖。
            gsap.to(el, {
              height: 0,
              opacity: 0,
              duration: COLLAPSE_DURATION,
              ease: 'power2.in',
              overwrite: 'auto',
            });
          },
          () => settle(false),
        );
        return stopMotion.current;
      }

      const rows = directRows(el, REVEAL_MAX_ROWS);
      const plan = revealPlan(rows.length);
      stopMotion.current = motionSafe(
        () => {
          // 层化提示只覆盖补间本身：留着它，这个容器会一直占一层合成层。
          gsap.set(el, { willChange: 'height' });
          gsap.fromTo(
            el,
            { height: 0, opacity: 0 },
            {
              height: 'auto',
              opacity: 1,
              duration: EXPAND_DURATION,
              ease: 'power2.out',
              overwrite: 'auto',
              onComplete: () => gsap.set(el, { willChange: 'auto' }),
            },
          );
          if (plan.rows > 1) {
            gsap.from(rows, {
              opacity: 0,
              y: REVEAL_Y,
              duration: REVEAL_DURATION,
              stagger: { each: plan.each, amount: plan.amount },
              ease: 'power2.out',
              overwrite: 'auto',
            });
          }
        },
        () => {
          settle(true);
          gsap.set(rows, { opacity: 1, y: 0 });
        },
      );
      return stopMotion.current;
    },
    { dependencies: [expanded], scope: containerRef },
  );

  return (
    <div
      ref={containerRef}
      data-tree-children=""
      style={{ overflow: 'hidden' }}
      aria-hidden={!expanded}
    >
      {children}
    </div>
  );
}
