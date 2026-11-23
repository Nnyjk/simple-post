/**
 * WorkspaceTabs —— 右栏上方的横向标签条。每个标签对应一个已打开的实体（项目 / 模块 /
 * 集合 / 接口），点击激活，「×」关闭，中键关闭同浏览器。
 *
 * - 底层实体（或其父级）被删的标签会在每次状态变化时自动剪除；同一清理也以
 *   `pruneStaleTabs` 暴露给绕过 delete* 动作的变更路径。
 * - 标签宽度**恒定 160px**，不随标签数变化。标签多到一条放不下时，条**横向滚动**，
 *   底部给一条覆盖式的细滚动条（3px），滑块可拖动。
 * - 尾部「关闭其他标签」按钮在滚动区**之外**，故内容溢出时它仍固定在右侧。
 * - 位置由 GSAP Flip 补间、横向滚动由 GSAP 补间，两条通道各管各的；宽度恒定，不参与
 *   动画。
 * - 激活标签与下方面板融合（浏览器惯例，见 `TabJoinCorners`），且条上只有它的标签用
 *   加粗字重。
 * - 标签位置在增删 / 重排时滑动而非瞬移（GSAP Flip，见下方 useLayoutEffect）。
 * - 激活标签下方的 0.5px 主色条是挂在条级的单一常驻元素，几何逐帧从激活标签的实时
 *   rect **推导**，见下方「Active-tab indicator」。
 */

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react';
import { X, ScrollText } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { Button } from '@/components/ui/button';
import { Dialog } from '@/components/ui/dialog';
import { TabJoinCorners } from '@/components/ui/tabs';
import { Tooltip } from '@/components/ui/tooltip';
import { cn, focusRingInset } from '@/lib/utils';
import { Flip, gsap } from '@/lib/gsap';
import { useTabDescriptors } from './useTabDescriptors';
import { scrollTargetForActiveTab, scrollbarThumb } from './tab-scroll';
import { nextTabFocusIndex } from './tab-keyboard';
import { TabContextMenu } from './TabContextMenu';

/**
 * 激活标签条的调参，理由见组件内「Active-tab indicator」。
 *
 * `BAR_APPROACH` 是每帧弥合剩余距离的比例：60fps 下 0.24 在 17 帧后只剩约 1%，即约
 * 0.28s 落定 —— 正好与 Flip 时长一致。
 *
 * `BAR_SETTLE_MS` 是变化之后继续读实时 rect 的时长，必须长过任何能移动标签的动画：
 * 280ms 的 Flip。标签宽度恒定，不参与动画，故窗口只需盖住这一段。
 *
 * 缓动还是直接就位，只看标签在 `openTabs` 里的**下标距离**，不看像素距离：横条压不压
 * 住中间标签的名字是标签相邻性问题，不是距离问题。用户要求减弱动效时一律直接就位，
 * 不论下标距离。
 */
const BAR_APPROACH = 0.24;
const BAR_SETTLE_MS = 480;

/**
 * 用户是否向系统要求减弱动效。
 *
 * 只在调用点判一次：给每个补间套 `motionSafe` 会为每个标签建一个 `matchMedia` 实例。
 * 渲染器里 `window.matchMedia` 恒可用，故不加 `typeof window` 守卫。
 */
const prefersReducedMotion = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;

export function WorkspaceTabs() {
  const openTabs = useAppStore((s) => s.openTabs);
  const activeTabId = useAppStore((s) => s.activeTabId);
  const dirtyTabIds = useAppStore((s) => s.dirtyTabIds);
  const setActiveTab = useAppStore((s) => s.setActiveTab);
  const requestCloseTab = useAppStore((s) => s.requestCloseTab);
  const requestCloseOthers = useAppStore((s) => s.requestCloseOthers);
  const requestCloseLeft = useAppStore((s) => s.requestCloseLeft);
  const requestCloseRight = useAppStore((s) => s.requestCloseRight);
  const requestCloseAllTabs = useAppStore((s) => s.requestCloseAllTabs);
  const closeSavedTabs = useAppStore((s) => s.closeSavedTabs);
  const pruneStaleTabs = useAppStore((s) => s.pruneStaleTabs);

  // 右键菜单，锚在被点的那个标签上；在点击位置渲染一个浮动菜单，关闭由外部点击 / Esc
  // 处理。
  const [menu, setMenu] = useState<{ tabId: string; x: number; y: number } | null>(null);

  // 「关闭已保存」一个都没关掉时的提示。
  const [noSavedTabsNotice, setNoSavedTabsNotice] = useState(false);

  // 额外的一轮清理，覆盖所有绕过 delete* 动作的变更路径。代价可接受：数据量以工作区规模
  // 为界。`nodesLoaded` 是依赖而非顺带捕获：`pruneStaleTabs` 在树加载完之前空操作
  // （空 `nodes` 与「全部被删」无法区分），不在这个翻转上重跑，刚进入的项目的标签就永远
  // 不会被剪掉。
  const nodesLoaded = useAppStore((s) => s.nodesLoaded);
  useEffect(() => {
    pruneStaleTabs();
  }, [pruneStaleTabs, nodesLoaded]);

  // 为每个标签建描述符。本地 useMemo，故每个标签只在自己底层实体变化时重渲染。
  const descriptors = useTabDescriptors(openTabs);

  // 标签加得多时把激活标签保持在可见区。
  const stripRef = useRef<HTMLDivElement>(null);
  // 滚动区节点本身进 state：它在最后一个标签关闭时整条卸载、下一条标签出现时重新挂载，
  // 而 ref 的变化不触发副作用重跑 —— 只认 ref 的话重挂后的条上就没有滚动监听了。
  const [stripEl, setStripEl] = useState<HTMLDivElement | null>(null);
  const attachStrip = useCallback((node: HTMLDivElement | null) => {
    stripRef.current = node;
    setStripEl(node);
  }, []);

  // 上一次记录的标签布局 —— Flip 补间的起点。
  const flipStateRef = useRef<ReturnType<typeof Flip.getState> | null>(null);

  // Flip 补间是否正在跑。补间进行中量到的是动画中途的位置，此时既不能重新记录布局，
  // 也不能按当前位置算滚动目标。
  const flipRunningRef = useRef(false);

  // 上一次补间是不是被打断的（下一轮补间紧接着开始）。被打断时不要在落定时滚动：
  // 下一轮的补间马上开始，两者会抢同一个 scrollLeft。
  const interruptedRef = useRef(false);

  // 「把激活标签滚进视野」的最新实现。用 ref 存，是因为 Flip 补间的 `onComplete`
  // 回调不该闭包住过期的激活标签 id。
  const scrollActiveIntoViewRef = useRef<() => void>(() => {});

  // 覆盖式滚动条的滑块。滑块几何逐帧写，故它是一个 ref 而不是状态：滚一次就重渲染
  // 整条标签，代价太大。
  const thumbRef = useRef<HTMLDivElement>(null);

  // 滚动条几何的最新实现。用 ref 存，是因为 Flip 落定的回调不该闭包住上一轮的实现 ——
  // 同 `scrollActiveIntoViewRef` 的理由。
  const syncScrollbarRef = useRef<() => void>(() => {});
  // 条能否横向滚动。滑块的可见性由它决定（状态），几何由 `syncScrollbar` 逐帧写。
  const [overflowing, setOverflowing] = useState(false);
  // 上面那个状态的镜像：滚动事件每帧都来，只在翻转时改状态。
  const overflowRef = useRef(false);

  // ------------------------------------------------------------------
  //  Tab position animation (GSAP Flip, FLIP pattern)
  // ------------------------------------------------------------------
  // 顺序是「提交前记下旧位置 → 提交 → 从旧位置补间到新位置」。React 的渲染阶段拿不
  // 到旧位置（那时新 DOM 还没提交），故旧位置由上一次记录的 `flipStateRef` 提供：
  // `Flip.getState` 取到的是本次提交后的最终布局，它与上一次记录的那份一比，就是
  // 补间的两端。
  //
  // 依赖里只有 `openTabs`：只有一个宽度模式，宽度不参与补间，故少一次重录就少一次
  // 把上一轮的位置记错的机会。
  //
  // 清理函数让上一次补间立刻落定：Flip 的补间期间把标签移出文档流
  //（`absolute: true`），不落定的话下一轮 `getState` 量到的是动画中途的位置。
  useLayoutEffect(() => {
    const strip = stripRef.current;
    if (!strip) return;
    const targets = strip.querySelectorAll<HTMLElement>('[data-tab-id]');
    if (targets.length === 0) {
      flipStateRef.current = null;
      return;
    }
    const current = Flip.getState(targets);
    const previous = flipStateRef.current;
    flipStateRef.current = current;
    if (!previous) return;
    // 本次新出现的标签没有「上一次的位置」：Flip 会把它当成进入元素，让它留在文档
    // 流里。其余标签被 Flip 移出文档流之后，它会退回条的最左边，与第一个标签重叠。
    // 故把它的当前（最终）位置并进那份记录 —— 它就成了「位置没变」的普通元素，
    // 一样被移出文档流，一样待在最终的位置上。
    const known = new Set(previous.targets);
    const entering = Array.from(targets).filter((el) => !known.has(el));
    if (entering.length > 0) previous.add(Flip.getState(entering));
    interruptedRef.current = false;
    // 减弱动效时时长走 0：补间仍然成立，`onComplete` 照常跑，标签照样回到文档流并滚动。
    const tween = Flip.from(previous, {
      duration: prefersReducedMotion() ? 0 : 0.28,
      ease: 'power2.out',
      absolute: true,
      // 补间结束后标签才回到文档流，那一刻才算得出它们最终在哪，故滚动放在这里。
      onComplete: () => {
        flipRunningRef.current = false;
        // 被打断的那一次不滚：下一轮补间紧接着开始，滚动会与它抢同一个 scrollLeft。
        // 下一轮补间结束后由它自己滚。
        if (interruptedRef.current) {
          interruptedRef.current = false;
          return;
        }
        // 补间期间标签被移出文档流，条的内容宽度是塌的；落定后宽度才是真的，
        // 滚动条这时才重算。
        syncScrollbarRef.current();
        scrollActiveIntoViewRef.current();
      },
    });
    flipRunningRef.current = true;
    return () => {
      // 补间被打断：让它立刻落定，标签才回到文档流，下一轮量到的才是最终位置。
      // 这里不能加 suppressEvents —— 那会把 GSAP 还原标签样式的回调一起抑制掉，
      // 标签会停在绝对定位上，条的内容宽度从此算不对。
      interruptedRef.current = true;
      tween.progress(1);
      flipRunningRef.current = false;
    };
  }, [openTabs]);

  // ------------------------------------------------------------------
  //  Keeping the recorded layout in sync with the scroll
  // ------------------------------------------------------------------
  // Flip 记的是**视口坐标**。条一旦滚动（程序滚动、滚轮滚动、浏览器自己纠正滚动
  // 位置），那份记录就过期了：下一次增删标签时，Flip 会从过期的视口坐标起步，
  // 整条标签先横向跳一截，再滑回正确位置。
  //
  // 故滚动停稳之后重新记一次。重新记录只在条静止时做 —— 补间进行中量到的是动画
  // 中途的位置，记下来比不记还糟。
  /**
   * 重新记录标签布局。
   *
   * Flip 记的是视口坐标，条滚动过之后那份记录就过期了 —— 下一次增删标签时，Flip 会
   * 从过期的视口坐标起步，整条标签先横向跳一截，再滑回正确位置。故滚动停稳之后要
   * 重新记一次。
   *
   * 补间进行中不记：那时量到的是动画中途的位置，记下来比不记更糟。
   */
  const captureFlipState = useCallback(() => {
    if (flipRunningRef.current) return;
    const strip = stripRef.current;
    if (!strip) return;
    const targets = strip.querySelectorAll<HTMLElement>('[data-tab-id]');
    flipStateRef.current = targets.length > 0 ? Flip.getState(targets) : null;
  }, []);

  // 滚轮连发时每滚一下都重新记录太贵（每次都要量一遍所有标签），故等滚轮停下来再记。
  const wheelRefreshTimerRef = useRef(0);
  /**
   * 稍后重新记录一次标签布局。
   *
   * 滚轮连发时每滚一下都重新记录太贵（每次都要把每个标签量一遍），故等滚轮停下来
   * 再记。
   */
  const scheduleFlipStateRefresh = useCallback(() => {
    window.clearTimeout(wheelRefreshTimerRef.current);
    wheelRefreshTimerRef.current = window.setTimeout(() => {
      captureFlipState();
    }, 120);
  }, [captureFlipState]);

  useEffect(() => {
    return () => window.clearTimeout(wheelRefreshTimerRef.current);
  }, []);

  // ------------------------------------------------------------------
  //  Active-tab indicator
  // ------------------------------------------------------------------
  // 逐帧推导，绝不量一次。
  //
  // 有两件互不相干的事会移动这些标签，且「量一次激活标签再补间过去」的读数时刻，二者都可能
  // 仍在飞行中：
  //
  //   1. 上面的 Flip。`absolute: true` 会把被补间的标签放到它**提交前**的位置，故补间期间
  //      读到的 `offsetLeft` 是动画位置而非最终位置。
  //   2. 条自己的 `scrollLeft`（滚轮处理与补间滚动都会动它）。
  //
  // 所以只要这两者之一可能在跑，条的目标就每帧从实时 rect 重读。当前位置再以固定比例向该
  // 目标缓动，于是条既能滑向相邻标签，也能跟住动画中的标签。
  //
  // 但缓动只用于**相邻标签**：跨过中间标签的缓动，那条横条会依次压过它们的
  // 名字，读起来就是「效果条乱跑 + 卡」，故换到非相邻标签就直接就位。用户要求减弱动效时
  // 一律直接就位，不看相邻性。
  //
  // 循环不是常驻的：任何能移动标签的东西变化时（激活标签、标签集）重跑一个
  // 有时限的窗口，窗口结束即停。窗口长到覆盖 280ms 的 Flip。
  const barRef = useRef<HTMLSpanElement>(null);
  // `started` 区分「放置」与「移动」：某次激活的首帧是直接就位（没有值得动画的上一位置 ——
  // 全新挂载，或条从无标签状态回来），之后每帧才缓动或就位。
  const barState = useRef({ x: 0, started: false });
  // 上一次放置时的激活标签：这一次该缓动还是直接就位，由下标距离决定 —— 缓动只在「换到
  // 相邻标签」时成立，跨标签的横扫会压住中间标签的名字。
  const prevBarKey = useRef<{ tabId: string | null }>({ tabId: null });

  const placeBar = useCallback(
    (reduced: boolean) => {
      const strip = stripRef.current;
      const bar = barRef.current;
      if (!strip || !bar) {
        // 标签在最后一个关闭时整条卸载（上面的 `descriptors.length === 0` 返回 null）。此处
        // 忘掉上一位置，下一条出现的条就会直接就位到自己的标签上，而不是从过期坐标滑进来。
        barState.current.started = false;
        prevBarKey.current = { tabId: null };
        return;
      }
      const activeEl = activeTabId
        ? strip.querySelector<HTMLElement>(`[data-tab-id="${activeTabId}"]`)
        : null;
      if (!activeEl) {
        // 没有任何激活项（全部关闭）—— 收起条，而不是让它停在一个已经不存在的标签下。
        barState.current.started = false;
        prevBarKey.current = { tabId: null };
        gsap.set(bar, { width: 0, opacity: 0 });
        return;
      }
      const stripRect = strip.getBoundingClientRect();
      const tabRect = activeEl.getBoundingClientRect();
      // 条是绝对定位在滚动容器**内部**的，故它的 x 是内容坐标系 —— 必须把滚动偏移加回来，
      // 否则条正好偏移一个已滚动的量。
      const targetX = tabRect.left - stripRect.left + strip.scrollLeft;
      const targetW = tabRect.width;
      const cur = barState.current;
      const prev = prevBarKey.current;
      const prevIndex = prev.tabId ? openTabs.findIndex((t) => t.id === prev.tabId) : -1;
      const nextIndex = openTabs.findIndex((t) => t.id === activeTabId);
      // 相邻（含同一个）才缓动：跨两个以上标签的缓动 = 横条擦过中间标签。
      const adjacent = prevIndex >= 0 && nextIndex >= 0 && Math.abs(nextIndex - prevIndex) <= 1;
      if (cur.started && !reduced) {
        const dx = targetX - cur.x;
        if (adjacent) cur.x += dx * BAR_APPROACH;
        else cur.x = targetX;
        if (Math.abs(targetX - cur.x) < 0.25) cur.x = targetX;
      } else {
        cur.x = targetX;
        cur.started = true;
      }
      prevBarKey.current = { tabId: activeTabId };
      // 宽度恒定（160px），不参与动画：每帧直接取目标的宽度。
      gsap.set(bar, { x: cur.x, width: targetW, opacity: 1 });
    },
    [activeTabId, openTabs],
  );

  useEffect(() => {
    // 减弱动效时条只需放一次，不进逐帧窗口；仍排到下一个动画帧，读到的才是 Flip 落定后的
    // 最终位置。
    if (prefersReducedMotion()) {
      const raf = requestAnimationFrame(() => placeBar(true));
      return () => cancelAnimationFrame(raf);
    }
    let raf = 0;
    const startedAt = performance.now();
    const tick = (now: number) => {
      placeBar(false);
      if (now - startedAt < BAR_SETTLE_MS) raf = requestAnimationFrame(tick);
    };
    // 首帧排到下一个动画帧而非同步执行：那是 Flip 真正开始的第一个时刻，条读到的才是真实
    // 位置，而不是一个即将被动画带走的已提交位置。
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
    // 依赖里列 `openTabs`（不只是它的 length）：关掉一个**非**激活
    // 标签时 `activeTabId` 不变，但它右侧的所有标签都会左滑。
  }, [placeBar, openTabs]);

  // ------------------------------------------------------------------
  //  Keeping the active tab visible
  // ------------------------------------------------------------------
  // 原生 `scrollIntoView` 会**瞬间**改掉 `scrollLeft`：所有标签的视口位置在同一帧
  // 里挪一截，那一下不参与任何动画，读起来就是「标签条猛地跳一下」。故滚动由 GSAP
  // 补间完成，该滚到哪儿由 `tab-scroll.ts` 算出。
  /**
   * 把激活标签滚进可视范围。
   *
   * 该滚到哪儿由 `scrollTargetForActiveTab` 算出，滚动本身由 GSAP 补间完成 ——
   * 原生 `scrollIntoView` 会瞬间改掉 `scrollLeft`，所有标签的视口位置在同一帧里
   * 挪一截，那一下不参与任何动画。
   */
  const scrollActiveIntoView = useCallback(() => {
    // Flip 补间期间标签停在动画中途的位置上，这时量出来的滚动目标是错的。守卫放在这里
    // 而不是只放在下面那个效果里：resize 也会调它，那条路径同样要挡住。
    if (flipRunningRef.current) return;
    const strip = stripRef.current;
    if (!strip || !activeTabId) return;
    const el = strip.querySelector<HTMLElement>(`[data-tab-id="${activeTabId}"]`);
    if (!el) return;
    const target = scrollTargetForActiveTab(strip, el.getBoundingClientRect());
    if (target === null) {
      // 激活标签本来就在可视范围里，但上一次滚动可能还在往别处跑：停掉它，否则条会
      // 继续滑，把刚点开的标签带出视野。
      gsap.killTweensOf(strip);
      captureFlipState();
      return;
    }
    if (prefersReducedMotion()) {
      // 减弱动效时直接就位：滚动量在这里仍然要落到目标值，只是不补间。
      gsap.set(strip, { scrollLeft: target });
      captureFlipState();
      return;
    }
    gsap.to(strip, {
      scrollLeft: target,
      duration: 0.28,
      ease: 'power2.out',
      overwrite: true,
      // 滚动改动了视口坐标，滚完之后重新记一次布局。
      onComplete: () => captureFlipState(),
    });
  }, [activeTabId, captureFlipState]);

  useEffect(() => {
    scrollActiveIntoViewRef.current = scrollActiveIntoView;
  }, [scrollActiveIntoView]);

  // 什么时候滚：标签集变了（增删、换项目）→ 由 Flip 补间落定后的 `onComplete` 滚，
  // 那时标签已回到文档流，位置才是最终位置；只是切换激活标签 → 没有 Flip 参与，
  // 这里立即滚（Flip 进行中时 `scrollActiveIntoView` 自己会拒绝）。
  const prevOpenTabsRef = useRef(openTabs);
  useEffect(() => {
    const tabsChanged = prevOpenTabsRef.current !== openTabs;
    prevOpenTabsRef.current = openTabs;
    if (tabsChanged) return;
    scrollActiveIntoView();
  }, [activeTabId, openTabs, scrollActiveIntoView]);

  // 滚轮转横向滚动，与各浏览器标签栏一致：竖轮给 `deltaY`，触控板的横向手势只给
  // `deltaX`，故取主导轴那个量。`overflow-hidden` 关掉了原生滚动，不接 `deltaX` 时
  // 横滑手势什么也不会发生。
  useEffect(() => {
    const el = stripEl;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
      if (delta === 0) return;
      if (el.scrollWidth <= el.clientWidth) return;
      e.preventDefault();
      // 程序滚动那个补间每帧都在写 scrollLeft，会把滚轮的结果抹掉：先停掉它。
      gsap.killTweensOf(el);
      el.scrollLeft += delta;
      // 滚动改动了视口坐标，重新记一次布局。
      scheduleFlipStateRefresh();
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [stripEl, scheduleFlipStateRefresh]);
  // ------------------------------------------------------------------
  //  Overlay scrollbar
  // ------------------------------------------------------------------
  //  标签宽度恒定，标签多到一条放不下时只能横向滚动。原生滚动条会占掉标签的高度、并把
  //  分隔线挤歪，故改成一条**覆盖式**的细条：它浮在条的下缘，不参与布局。
  //
  //  几何由 `scrollbarThumb` 算出（纯函数，可单测），这里只负责把它写进 DOM。
  /**
   * 把滚动条滑块的几何写进 DOM。
   *
   * 内容放得下时（`scrollbarThumb` 返回 null）滑块隐藏 —— 看不见的滑块不该还能被拖。
   */
  const syncScrollbar = useCallback(() => {
    const strip = stripRef.current;
    const thumb = thumbRef.current;
    if (!strip || !thumb) return;
    // Flip 补间期间标签被移出文档流，条的内容宽度是塌的：这时算出来的几何是错的，
    // 还会让滑块在补间期间闪一下。补间落定后 `onComplete` 里会重算一次。
    if (flipRunningRef.current) return;
    const box = scrollbarThumb(strip);
    // 能否滚动只决定滑块**可不可见**（一次状态翻转），位置与宽度逐帧写 inline style。
    if (box === null) {
      if (overflowRef.current) {
        overflowRef.current = false;
        setOverflowing(false);
      }
      return;
    }
    if (!overflowRef.current) {
      overflowRef.current = true;
      setOverflowing(true);
    }
    thumb.style.left = `${box.left}px`;
    thumb.style.width = `${box.width}px`;
  }, []);

  useEffect(() => {
    syncScrollbarRef.current = syncScrollbar;
  }, [syncScrollbar]);

  // 滚动条要跟上三种变化：条自己滚了、标签集或激活标签变了、窗口尺寸变了。
  // 滚动补间逐帧改 `scrollLeft`，会逐帧触发 `scroll` 事件，故补间期间也不缺更新。
  //
  // 窗口变窄时条也跟着窄，激活标签可能被挤出可视区（标签宽度恒定，不会自己收窄），
  // 故 resize 一并把它滚回来。
  useEffect(() => {
    if (!stripEl) return;
    const onResize = () => {
      syncScrollbar();
      scrollActiveIntoViewRef.current();
    };
    stripEl.addEventListener('scroll', syncScrollbar);
    window.addEventListener('resize', onResize);
    return () => {
      stripEl.removeEventListener('scroll', syncScrollbar);
      window.removeEventListener('resize', onResize);
    };
  }, [stripEl, syncScrollbar]);

  useEffect(() => {
    syncScrollbar();
  }, [syncScrollbar, openTabs, activeTabId]);

  // 拖动滑块：指针按下后捕获它，故指针移出滑块仍然跟手。
  const dragStateRef = useRef<{
    startX: number;
    startScroll: number;
    thumbWidth: number;
  } | null>(null);
  /**
   * 按下滑块，开始拖动。
   */
  const onThumbPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    const el = stripRef.current;
    if (!el) return;
    e.preventDefault();
    e.stopPropagation();
    // 程序滚动那个补间每帧都在写 scrollLeft，会把拖动的结果抹掉：先停掉它。
    gsap.killTweensOf(el);
    dragStateRef.current = {
      startX: e.clientX,
      startScroll: el.scrollLeft,
      thumbWidth: thumbRef.current?.getBoundingClientRect().width ?? 0,
    };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  /**
   * 拖动滑块：把指针的横向位移折算成滚动量。
   */
  const onThumbPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragStateRef.current;
    const el = stripRef.current;
    if (!drag || !el) return;
    const maxScroll = el.scrollWidth - el.clientWidth;
    const travel = el.clientWidth - drag.thumbWidth;
    if (travel <= 0) return;
    // 拖动是用户手势，与滚轮同一性质：直接写 scrollLeft，不补间。
    const next = drag.startScroll + ((e.clientX - drag.startX) * maxScroll) / travel;
    el.scrollLeft = Math.min(Math.max(next, 0), maxScroll);
    // 滚动改动了视口坐标，重新记一次布局。
    scheduleFlipStateRefresh();
  };
  /**
   * 松开滑块：结束拖动。
   */
  const onThumbPointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    dragStateRef.current = null;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
  };

  // 方向键在标签之间移动**焦点**，不激活 —— 激活是标签自己的 Enter/Space，好让键盘用
  // 户能先看清落到哪个标签上（自动激活会在方向键连按时逐个拉取/渲染面板）。
  // 焦点落在标签内的关闭按钮上时 `closest` 同样命中，故从那里按方向键也能继续走。
  const onStripKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    const strip = stripRef.current;
    const current = (e.target as HTMLElement).closest<HTMLElement>('[data-tab-id]');
    if (!strip || !current) return;
    const all = Array.from(strip.querySelectorAll<HTMLElement>('[data-tab-id]'));
    const next = nextTabFocusIndex(e.key, all.indexOf(current), all.length);
    if (next < 0) return;
    e.preventDefault();
    all[next]?.focus();
  };

  if (descriptors.length === 0) return null;

  return (
    // 外层：分隔线与尾部按钮都在**滚动区之外**，故内容溢出时尾部按钮仍固定在右侧，
    // 滚动区的 clientWidth 也不含它。
    //
    // 这条标签条不用 `border-b`：分隔线是子元素而不是边框。本条按 padding box 裁剪，
    // 自带边框会落在裁剪之外，激活标签就无法与下方面板融合。
    <div className="relative flex h-9 shrink-0 items-stretch bg-card/40 pr-1.5">
      {/* 分隔线。`z-0` 让激活标签的填充（z-10）压过底下那条横线。 */}
      <span
        aria-hidden
        className="pointer-events-none absolute inset-x-0 bottom-0 z-0 h-px bg-border"
      />
      {/* 滚动区。`role="tablist"` 必须落在真正包含 `role="tab"` 的这一层。 */}
      <div
        ref={attachStrip}
        role="tablist"
        aria-label="工作区标签"
        onKeyDown={onStripKeyDown}
        className="relative flex h-full min-w-0 flex-1 items-stretch gap-0.5 overflow-hidden"
      >
        {descriptors.map((d) => {
          const active = d.tab.id === activeTabId;
          const dirty = dirtyTabIds.has(d.tab.id);
          // 悬停时给出的完整说明。接口标签额外带上它所在容器的路径——重名的接口靠这条路径
          // 分辨，标签条本身只放得下其中一段。
          const titleText = d.path
            ? `${d.title} · ${d.path}`
            : d.kindLabel
              ? `${d.kindLabel} · ${d.title}`
              : d.title;
          return (
            <div
              key={`${d.tab.kind}:${d.tab.id}`}
              data-tab-id={d.tab.id}
              role="tab"
              aria-selected={active}
              tabIndex={0}
              onKeyDown={(e) => {
                // 从标签内关闭按钮冒上来的 Enter 属于那个按钮，不能同时激活标签。
                if (e.target !== e.currentTarget) return;
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  setActiveTab(d.tab.id);
                } else if (e.key === 'Delete') {
                  // 与中键同义：关闭，但不激活。
                  e.preventDefault();
                  requestCloseTab(d.tab.id);
                }
              }}
              onMouseDown={(e) => {
                // 中键关闭标签（浏览器惯例）。
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
                // 宽度恒定：每个标签恰好 `w-[160px]`，条读起来是等宽单元格网格，标题约容
                // 5–6 个汉字后截断，过长的 hint 在标签内截成 `...`。标签多到一条放不下时
                // 条**横向滚动**（见下方「Overlay scrollbar」），标签本身不变窄。
                //
                // 这里**没有**宽度的 CSS 过渡：宽度恒定、不参与动画，位置由上方的 Flip 补间。
                'group relative flex h-full shrink-0 cursor-pointer items-center gap-1.5 rounded-t-md border-x border-t border-transparent px-2.5 text-xs',
                focusRingInset,
                'w-[160px]',
                active
                  ? 'border-border bg-background text-foreground'
                  : // 用 `bg-overlay-hover` 而非手写 `bg-accent/40`：新暗色阶梯上 `--accent`
                    // （17.6%）与条自身的底色几乎相同，实心 accent 的 hover 只差约一个亮度级，
                    // 读起来等于没变。overlay token 是一个真实台阶。
                    'text-muted-foreground hover:bg-overlay-hover hover:text-foreground',
                // `z-10` 是激活标签与面板融合的关键：它必须盖过条的分隔线，否则 `z-0` 的
                // 发丝线子元素会横穿它的底边。
                active && 'z-10',
              )}
              onClick={() => setActiveTab(d.tab.id)}
            >
              {/* 先切肩：切口必须擦掉标签自己的两个下角（填充*与*侧边框），
                所以要盖在它之上、又盖在任何用户看得见的东西之下。 */}
              {active && <TabJoinCorners stripClass="bg-card/40" />}
              <span className={cn('shrink-0', active && d.iconColor)}>{d.icon}</span>
              <span
                // 标题是唯一会伸展的元素，始终截断：上限 `max-w-[88px]`（≈ 5 个汉字），
                // 右侧留给 hint。完整标题通过 `title` 属性在 hover 时可得。
                //
                // 字重步进挂在标签文字上而不是整个标签：method pill 与旁边的图标不该跟着加粗。
                className={cn('min-w-0 flex-1 truncate max-w-[88px]', active && 'font-semibold')}
                title={titleText}
              >
                {d.title}
              </span>
              {/*
               * 尾部控件作为一个 flex item 整体出行，才能用 `ml-auto` 推到标签右缘。
               *
               * 标题带 `flex-1` 但又被 `max-w-[88px]` 封顶，这正是 hint 的空间来源。该上限也
               * 意味着短标题会留下 flexbox 无处安放的空隙，它会落在行的*末端*、关闭按钮之后。
               * `ml-auto` 把这段空隙移到这一组控件之前，也就是浏览器标签放它的位置：
               * 标题靠左、× 钉在右。
               *
               * 用一个包裹元素而不是直接给关闭按钮加 `ml-auto`：`Tooltip` 会把子元素
               * 包在 `relative inline-flex` 的 span 里，那个 span 才是 flex item。
               */}
              <span className="ml-auto inline-flex shrink-0 items-center gap-1.5">
                {dirty && (
                  <Tooltip content="有未保存的修改" side="bottom">
                    <span
                      className="h-1.5 w-1.5 shrink-0 rounded-full bg-warning"
                      aria-label="有未保存的修改"
                    />
                  </Tooltip>
                )}
                {d.hint && (
                  <Tooltip content={d.path || d.hint} side="bottom">
                    <span className="max-w-[40px] truncate text-nano text-faint">{d.hint}</span>
                  </Tooltip>
                )}
                <Tooltip content="关闭" side="bottom">
                  <button
                    type="button"
                    aria-label="关闭标签"
                    // 阻止冒泡，关闭点击不会顺带激活标签。
                    onClick={(e) => {
                      e.stopPropagation();
                      requestCloseTab(d.tab.id);
                    }}
                    className={cn(
                      'flex h-4 w-4 shrink-0 items-center justify-center rounded-sm text-faint hover:bg-overlay-hover hover:text-foreground',
                      // hover 时始终显示，但激活标签上保持可见；`group-focus-within` 让键盘
                      // 落到标签上时 × 也现形，否则它是个不可见的焦点目标。
                      active
                        ? 'opacity-100'
                        : 'opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100',
                    )}
                  >
                    <X className="h-3 w-3" />
                  </button>
                </Tooltip>
              </span>
              {/* 激活标签指示条移到条级元素（见下方），才能作为单一常驻节点在
                标签之间补间。 */}
            </div>
          );
        })}
        {/* 常驻的激活标签指示条。挂在条级以跨标签集变化作为单一元素存活，几何在布局运动
          期间逐帧写入 —— 见上方「Active-tab indicator」。class 里刻意不含 `w-0`：宽度完全
          由驱动逐帧写，class 级宽度只会变成与它打架的行内覆盖。 */}
        <span
          ref={barRef}
          aria-hidden
          className="pointer-events-none absolute left-0 top-0 z-20 h-0.5 rounded-t-md bg-primary opacity-0"
        />
      </div>

      {/* 覆盖式滚动条。浮在条的下缘，不占布局 —— 标签高度与分隔线都不受影响。

          它在滚动区**之外**：绝对定位的子元素会跟着内容一起滚走，放在里面的话滑块会
          随标签飘走。放在外面，它的位置就是纯视口坐标 —— 滑块的几何（left / width）
          由 `scrollbarThumb` 算出，它落在滚动区的可视范围内（宽度按滚动区的
          clientWidth 算，故不会伸到尾部按钮上）。

          滑块停在分隔线**上方** 2px：不压掉那条发丝线，也不切断激活标签与面板的贴合。 */}
      <div aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 z-30 h-2">
        <div
          ref={thumbRef}
          onPointerDown={onThumbPointerDown}
          onPointerMove={onThumbPointerMove}
          onPointerUp={onThumbPointerUp}
          onPointerCancel={onThumbPointerUp}
          className={cn(
            'absolute bottom-0.5 h-[3px] rounded-full bg-muted-foreground/35 transition-[height,background-color,opacity] duration-150 hover:h-[4px] hover:bg-muted-foreground/60',
            // 内容放得下时滑块整体消失，也不再接收指针 —— 看不见的东西不该还能被拖。
            overflowing ? 'pointer-events-auto opacity-100' : 'pointer-events-none opacity-0',
          )}
        />
      </div>

      {/* 尾部按钮在滚动区**之外**：内容溢出时它仍固定在右侧，滚动区的可视宽度也不含它。 */}
      <div className="ml-1 flex shrink-0 items-center">
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 text-faint"
          onClick={() => {
            // 「关闭其他标签」—— 深潜树后攒下几十个标签时有用。这里走 dirty 守卫，
            // 让另一个标签里改到一半的命名有机会先落定。
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
          onCloseOthers={() => {
            // 右键菜单的「关闭其他」以**被右键的那个标签**为基准，不是活动标签。
            requestCloseOthers(menu.tabId);
            setMenu(null);
          }}
          onCloseLeft={() => {
            requestCloseLeft(menu.tabId);
            setMenu(null);
          }}
          onCloseRight={() => {
            requestCloseRight(menu.tabId);
            setMenu(null);
          }}
          onCloseAll={() => {
            // `requestCloseAllTabs` 内部的 dirty 守卫会在任何标签有未保存修改时弹出
            // DirtyCloseDialog，故这里不需要额外的 window.confirm() 提示。
            requestCloseAllTabs();
            setMenu(null);
          }}
          onCloseSaved={() => {
            const n = closeSavedTabs();
            if (n === 0) setNoSavedTabsNotice(true);
            setMenu(null);
          }}
        />
      )}

      {/* 没有可关闭的已保存标签 —— 一条只讲事实的提示。
          用对话框而不是 `window.alert`：原生弹窗的样式不受控，在 Tauri 的 webview 里
          还会盖住整个应用窗口。 */}
      <Dialog
        open={noSavedTabsNotice}
        onOpenChange={setNoSavedTabsNotice}
        title="没有可关闭的已保存标签"
        description="其余标签都有未保存的修改，关掉它们需要先确认。"
        footer={
          <Button variant="ghost" onClick={() => setNoSavedTabsNotice(false)}>
            知道了
          </Button>
        }
      />
    </div>
  );
}
