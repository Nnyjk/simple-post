/**
 * 渲染器里唯一初始化 GSAP 的地方。
 *
 * - `gsap.registerPlugin(...)` 必须在任何动画代码执行前跑过一次。集中在这里，
 *   各组件一律从本模块 import `gsap` / `useGSAP` / `Flip`，注册就白拿：不会漏注册，
 *   不会重复注册，dev 下也没有顺序意外。
 * - 把 gsap 与 @gsap/react 挡在其他代码的 import 面之外，也让日后换掉或移除变简单
 *   （删掉本文件与依赖即可，别处不会坏）。
 *
 * 注意：不要从任何会在 SSR 期执行的地方 import 本文件。本应用是 Tauri 2 webview，
 * 不涉及 SSR；万一变了，就用 `typeof window !== 'undefined'` 守卫。
 */
import gsap from 'gsap';
import { useGSAP } from '@gsap/react';
import { Flip } from 'gsap/Flip';

// 模块求值时注册一次。插件注册是幂等的（`registerPlugin` 调两次是空操作），
// 但放在这里能让每个消费方的心智模型只有一句话：「我从 '@/lib/gsap' 导入，拿到的
// 是接好线的插件。」
gsap.registerPlugin(useGSAP, Flip);

/**
 * 仅当用户没有向系统要求减弱动效时才播动画。返回清理函数，以便与 `useGSAP`
 * 自己的返回清理契约组合：
 *
 * ```ts
 * useGSAP(() => {
 *   if (!panelRef.current) return;
 *   return motionSafe(() => {
 *     gsap.fromTo(panelRef.current, { opacity: 0 }, { opacity: 1 });
 *   });
 * }, [open]);
 * ```
 *
 * `jumpToEnd` 用于**承载状态**而非装饰性的动画——传入「完全不动时应该是什么」：
 *
 * ```ts
 * return motionSafe(() => gsap.to(el, { height: 0 }), () => gsap.set(el, { height: 0 }));
 * ```
 *
 * 这个区别是要点，弄错会弄坏功能而不只是观感不同。收起的树分支由 GSAP 持有的
 * 内联 `height: 0` 关着，所以单纯不播展开补间，会让分支在用户展开后仍然不可见。
 * 装饰可以跳过，状态必须落到某个值上。
 *
 * 为什么不在每个调用点用 `gsap.matchMedia()`：`matchMedia` 才是对的原语，但有用的
 * 是那些 `(prefers-reduced-motion: …)` 字符串与 `revert()`，而这两样都容易漏写。
 * 统一一种写法，能让「我们尊重这个」到处为真，或者到处明显为假。
 *
 * 注意查询条件是 `no-preference` / `reduce` 双边而非单边：默认播动画、退出显式，
 * 而两边都不认识的浏览器什么都不播，而不是无条件播。
 */
export function motionSafe(animate: () => void, jumpToEnd?: () => void): () => void {
  const mm = gsap.matchMedia();
  mm.add('(prefers-reduced-motion: no-preference)', animate);
  if (jumpToEnd) mm.add('(prefers-reduced-motion: reduce)', jumpToEnd);
  return () => mm.revert();
}

export { gsap, useGSAP, Flip };
