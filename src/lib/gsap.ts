/**
 * 渲染器里唯一初始化 GSAP 的地方。
 *
 * - `gsap.registerPlugin(...)` 必须在任何动画代码执行前跑过一次。各组件一律从本模块
 *   import `gsap` / `useGSAP` / `Flip`，故注册只发生一次：不会漏注册，不会重复注册，
 *   dev 下也没有顺序意外。
 * - 不要从任何会在 SSR 期执行的地方 import 本文件。本应用是 Tauri 2 webview，
 *   不涉及 SSR；万一变了，就用 `typeof window !== 'undefined'` 守卫。
 */
import gsap from 'gsap';
import { useGSAP } from '@gsap/react';
import { Flip } from 'gsap/Flip';

// 模块求值时注册一次。`registerPlugin` 调两次是空操作，放在这里能让每个消费方
// 只记住一件事：从 '@/lib/gsap' 导入，拿到的就是已经注册好的插件。
gsap.registerPlugin(useGSAP, Flip);

/**
 * 仅当用户没有向系统要求减弱动效时才播动画。返回清理函数，以便与 `useGSAP`
 * 自己的返回清理契约组合。
 *
 * - `jumpToEnd` 用于**承载状态**而非装饰性的动画，传入「完全不动时应该是什么」。
 *   收起的树分支由 GSAP 持有的内联 `height: 0` 关着，故单纯不播展开补间会让分支在
 *   用户展开后仍然不可见：装饰可以跳过，状态必须落到某个值上。
 * - 不用 `gsap.matchMedia()`：`matchMedia` 才是对的原语，但有用的
 *   `(prefers-reduced-motion: …)` 字符串与 `revert()` 都容易漏写；统一一种写法，
 *   「我们尊重这个」才能到处为真，或者到处明显为假。
 * - 查询条件是 `no-preference` / `reduce` 双边而非单边：默认播动画、退出显式，
 *   而两边都不认识的浏览器什么都不播。
 */
export function motionSafe(animate: () => void, jumpToEnd?: () => void): () => void {
  const mm = gsap.matchMedia();
  mm.add('(prefers-reduced-motion: no-preference)', animate);
  if (jumpToEnd) mm.add('(prefers-reduced-motion: reduce)', jumpToEnd);
  return () => mm.revert();
}

export { gsap, useGSAP, Flip };
