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
 * 「减弱动效」的唯一判定。
 *
 * 整份应用共用这一个媒体查询实例：调用点各建一个 `gsap.matchMedia()` 时，几百行的树就是
 * 几百个实例，全部在挂载时付这笔钱。`matches` 随系统设置实时变化，故不缓存布尔值。
 */
const REDUCED_MOTION_QUERY = '(prefers-reduced-motion: reduce)';
let reducedMotionQuery: MediaQueryList | null = null;

function reducedMotion(): MediaQueryList {
  reducedMotionQuery ??= window.matchMedia(REDUCED_MOTION_QUERY);
  return reducedMotionQuery;
}

/** 用户是否向系统要求减弱动效。 */
export function prefersReducedMotion(): boolean {
  return reducedMotion().matches;
}

/**
 * 仅当用户没有向系统要求减弱动效时才播动画。返回清理函数，以便与 `useGSAP`
 * 自己的返回清理契约组合。
 *
 * - `jumpToEnd` 用于**承载状态**而非装饰性的动画，传入「完全不动时应该是什么」。
 *   收起的树分支由 GSAP 持有的内联 `height: 0` 关着，故单纯不播展开补间会让分支在
 *   用户展开后仍然不可见：装饰可以跳过，状态必须落到某个值上。
 * - 判定只走 `prefersReducedMotion()` 背后那一个媒体查询，并订阅它的 `change`：用户
 *   中途改系统设置时，正在播的那一套立刻换成另一套，不用重新挂载组件。
 * - 默认播动画：查询不被支持的浏览器 `matches` 为假，走 `animate` 那一支。
 */
export function motionSafe(animate: () => void, jumpToEnd?: () => void): () => void {
  const query = reducedMotion();
  const apply = () => {
    if (query.matches) jumpToEnd?.();
    else animate();
  };
  apply();
  query.addEventListener('change', apply);
  return () => query.removeEventListener('change', apply);
}

export { gsap, useGSAP, Flip };
