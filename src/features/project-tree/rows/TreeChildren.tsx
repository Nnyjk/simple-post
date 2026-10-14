import { useRef, type ReactNode } from 'react';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';

/**
 * 包裹树节点的子级（模块 → CollectionBranch，或 CollectionRow → 接口 + 子集合），使展开 /
 * 折叠能用 GSAP 动画而非硬挂载 / 卸载。子级始终留在 DOM 中；可见性由容器的 `height` +
 * `opacity` 驱动。展开时还对其直接行后代做错峰淡入，让新内容逐行浮现而不是一次性弹出。
 */
export function TreeChildren({ expanded, children }: { expanded: boolean; children: ReactNode }) {
  const containerRef = useRef<HTMLDivElement>(null);
  // `useGSAP` 除每次依赖变化外，挂载时也会跑一次回调。不加守卫的话，初始即展开的分支会
  // 在首帧从 height: 0 补间到 'auto'，闪一下折叠态。故首次运行直接同步到当前状态，之后
  // 每次变化才做动画。
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
      // 这个过渡既是动效，也是本容器在开合之间唯一的位移来源，故 reduced motion 必须落到
      // 终态而不是跳过 —— 折叠高度是 GSAP 拥有的内联样式，跳过补间会让用户刚展开的分支停在
      // `height: 0`。
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
