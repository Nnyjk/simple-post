/**
 * WorkspaceTabs —— 右栏上方的横向标签条。每个标签对应一个已打开的实体
 * （项目 / 模块 / 集合 / 接口），点击激活，「×」关闭，中键关闭同浏览器。
 *
 * - 底层实体（或其父级）被删的标签会在每次状态变化时自动剪除；同一清理也
 *   以 `pruneStaleTabs` 暴露给绕过 delete* 动作的变更路径。
 * - 标签放不下时进入 **compact 模式**，不出横向滚动条（会破坏周边 chrome）。
 *   两种模式都是**固定宽度**（normal=160px、compact=92px），条内每个标签
 *   等宽，读起来是一张干净的网格；标题在 normal 下截到 ≈ 5–6 个汉字、compact
 *   下 ≈ 2 个，hint 只在 normal 下显示并受 `max-w` 限制。宽度过渡走 CSS，
 *   位置滑动走 GSAP Flip；标签集、激活标签或窗口尺寸变化时重新测量。
 * - 激活标签与下方面板融合（浏览器惯例，见 `TabJoinCorners`），且条上只有它
 *   的标签用加粗字重 —— 字重步进取自 WinShun 的 `PageTabs.qml`，这里不需要
 *   它的宽度预留 trick，因为标签宽度固定、标题截断而非回流。
 * - 标签位置在增删/重排时滑动而非瞬移（GSAP Flip，见下方 useGSAP）。
 * - 激活标签下方的 0.5px 主色条是挂在条级的单一常驻元素，几何逐帧从激活标签
 *   的实时 rect **推导**，而不是量一次再补间 —— 见下方「Active-tab indicator」。
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { X, ScrollText } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { Button } from '@/components/ui/button';
import { TabJoinCorners } from '@/components/ui/tabs';
import { Tooltip } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { Flip, gsap, useGSAP } from '@/lib/gsap';
import { useTabDescriptors } from './useTabDescriptors';
import { TabContextMenu } from './TabContextMenu';

/**
 * 激活标签条的调参，理由见组件内「Active-tab indicator」。
 *
 * `BAR_APPROACH` 是每帧弥合剩余距离的比例：60fps 下 0.24 在 17 帧后只剩约 1%，
 * 即约 0.28s 落定 —— 正好与 Flip 时长一致，两者才像同一段动作。
 *
 * `BAR_SETTLE_MS` 是变化之后继续读实时 rect 的时长，必须长过任何能移动标签的
 * 动画：280ms Flip + 200ms CSS 宽度过渡，并留出 resize 连续触发两者的余量。
 */
const BAR_APPROACH = 0.24;
const BAR_SETTLE_MS = 480;
export function WorkspaceTabs() {
  const openTabs = useAppStore((s) => s.openTabs);
  const activeTabId = useAppStore((s) => s.activeTabId);
  const dirtyTabIds = useAppStore((s) => s.dirtyTabIds);
  const setActiveTab = useAppStore((s) => s.setActiveTab);
  const requestCloseTab = useAppStore((s) => s.requestCloseTab);
  const requestCloseOthers = useAppStore((s) => s.requestCloseOthers);
  const requestCloseAllTabs = useAppStore((s) => s.requestCloseAllTabs);
  const closeSavedTabs = useAppStore((s) => s.closeSavedTabs);
  const pruneStaleTabs = useAppStore((s) => s.pruneStaleTabs);

  // 右键菜单，锚在被点的那个标签上；在点击位置渲染一个浮动菜单，
  // 关闭由外部点击 / Esc 处理。
  const [menu, setMenu] = useState<{ tabId: string; x: number; y: number } | null>(null);

  // 额外的一轮清理，覆盖所有绕过 delete* 动作的变更路径。代价可接受：数据量以
  // 工作区规模为界。
  //
  // `nodesLoaded` 是依赖而非顺带捕获：`pruneStaleTabs` 在树加载完之前空操作
  // （空 `nodes` 与"全部被删"无法区分），不在这个翻转上重跑，刚进入的项目的
  // 标签就永远不会被剪掉。
  const nodesLoaded = useAppStore((s) => s.nodesLoaded);
  useEffect(() => {
    pruneStaleTabs();
  }, [pruneStaleTabs, nodesLoaded]);

  // 为每个标签建描述符。本地 useMemo，故每个标签只在自己底层实体变化时重渲染。
  const descriptors = useTabDescriptors(openTabs);

  // 标签加得多时把激活标签保持在可见区。
  const stripRef = useRef<HTMLDivElement>(null);

  // compact 模式的判定：条里放不下全宽标签时，让每个标签缩到更小的下限
  // （图标 + 4-5 字 + 关闭），而不是出横向滚动条 —— 后者会与周边 chrome 打架。
  // 标题靠 `max-w` 截断，绝不退化成纯图标（眼睛在标签里总该看到点文字）。
  //
  // 两种模式都是固定宽度（normal=160px、compact=92px），条内看起来一致。
  // 阈值按"能放下多少个 *normal* 标签"计算，放不下就整体降到 compact 宽度。
  //
  // 标签集变化、激活标签切换、窗口 resize 时重新测量；用 resize 监听而非
  // ResizeObserver，因为只需要条自己的 clientWidth，且 resize 每帧至多一次。
  const [isCompact, setIsCompact] = useState(false);
  const measureCompact = useCallback(() => {
    const el = stripRef.current;
    if (!el) return;
    const NORMAL_TAB = 160; // px, fixed width in normal mode
    const TRAILING = 76; // px reserved for the trailing "关闭其他" button + padding
    const tabs = el.querySelectorAll<HTMLElement>('[data-tab-id]');
    const minTotal = tabs.length * NORMAL_TAB;
    setIsCompact(minTotal > el.clientWidth - TRAILING);
  }, []);
  useLayoutEffect(() => {
    measureCompact();
  }, [measureCompact, activeTabId, openTabs.length]);
  useEffect(() => {
    window.addEventListener('resize', measureCompact);
    return () => window.removeEventListener('resize', measureCompact);
  }, [measureCompact]);

  // ------------------------------------------------------------------
  //  Tab position animation (GSAP Flip, FLIP pattern)
  // ------------------------------------------------------------------
  // useGSAP's `dependencies` + `revertOnUpdate: true` runs each callback
  // *after* React commits, then registers a cleanup that's invoked the
  // next time deps change. We capture tab positions in the callback and
  // play them back in the cleanup — that order is what gives us FLIP:
  // the new DOM has already been committed by the time the cleanup runs,
  // so `Flip.from(state)` can tween from the previously-captured positions
  // to the just-committed positions. `absolute: true` lets Flip take the
  // tabs out of flow during the tween so neighbours don't reflow as each
  // tab slides.
  useGSAP(
    () => {
      if (!stripRef.current) return;
      const targets = stripRef.current.querySelectorAll<HTMLElement>('[data-tab-id]');
      if (targets.length === 0) return;
      const state = Flip.getState(targets);
      return () => {
        Flip.from(state, {
          duration: 0.28,
          ease: 'power2.out',
          absolute: true,
        });
      };
    },
    { dependencies: [openTabs], scope: stripRef, revertOnUpdate: true },
  );

  // ------------------------------------------------------------------
  //  Active-tab indicator
  // ------------------------------------------------------------------
  // 逐帧推导，绝不量一次。
  //
  // 有三件互不相干的事会移动这些标签，且"量一次激活标签再补间过去"的读数
  // 时刻，三者都可能仍在飞行中：
  //
  //   1. 上面的 Flip。`absolute: true` 会把每个被补间的标签以 `position: absolute`
  //      放到它**提交前**的位置，所以补间期间读到的 `offsetLeft` 是动画位置而非
  //      最终位置 —— 照它补间，条会指向标签刚才所在的地方。
  //   2. 标签的 CSS 宽度过渡（`transition-[width,…]`，200ms），条在 normal 与
  //      compact 之间翻转时触发。整个过渡期间它既改每个标签的宽度，也改每个邻居
  //      的 offset。
  //   3. 条自己的 `scrollLeft`（滚轮处理与 `scrollIntoView` 都会动它）。
  //
  // 所以只要这三者之一可能在跑，条的目标就每帧从实时 rect 重读。当前位置再以
  // 固定比例向该目标缓动，于是条既能滑向相邻标签，也能跟住动画中的标签，
  // 且不会停在一个已经过期的数值上。
  //
  // 循环不是常驻的：任何能移动标签的东西变化时（激活标签、标签集、compact
  // 模式）重跑一个有时限的窗口，窗口结束即停。窗口长到覆盖 280ms Flip 与
  // 200ms 宽度过渡再加其上的 resize 连发，短到空闲的条不做逐帧工作。
  const barRef = useRef<HTMLSpanElement>(null);
  // `started` 区分"放置"与"移动"：某次激活的首帧是直接就位（没有值得动画的
  // 上一位置 —— 全新挂载，或条从无标签状态回来），之后每帧才缓动。
  const barState = useRef({ x: 0, width: 0, started: false });

  const placeBar = useCallback(() => {
    const strip = stripRef.current;
    const bar = barRef.current;
    if (!strip || !bar) {
      // 标签在最后一个关闭时整条卸载（上面的 `descriptors.length === 0` 返回 null）。
      // 此处忘掉上一位置，下一条出现的条就会直接就位到自己的标签上，而不是从
      // 一个过期坐标滑进来。
      barState.current.started = false;
      return;
    }
    const activeEl = activeTabId
      ? strip.querySelector<HTMLElement>(`[data-tab-id="${activeTabId}"]`)
      : null;
    if (!activeEl) {
      // 没有任何激活项（全部关闭）—— 收起条，而不是让它停在一个已经不存在的标签下。
      barState.current.started = false;
      gsap.set(bar, { width: 0, opacity: 0 });
      return;
    }
    const stripRect = strip.getBoundingClientRect();
    const tabRect = activeEl.getBoundingClientRect();
    // 条是绝对定位在滚动容器**内部**的，故它的 x 是内容坐标系 —— 必须把滚动
    // 偏移加回来，否则条正好偏移一个已滚动的量。
    const targetX = tabRect.left - stripRect.left + strip.scrollLeft;
    const targetW = tabRect.width;
    const cur = barState.current;
    if (cur.started) {
      // 指数逼近：约 0.28s 落定 —— 与 Flip 的时长一致，但目标本身可能正在移动。
      cur.x += (targetX - cur.x) * BAR_APPROACH;
      cur.width += (targetW - cur.width) * BAR_APPROACH;
      if (Math.abs(targetX - cur.x) < 0.25 && Math.abs(targetW - cur.width) < 0.25) {
        cur.x = targetX;
        cur.width = targetW;
      }
    } else {
      cur.x = targetX;
      cur.width = targetW;
      cur.started = true;
    }
    gsap.set(bar, { x: cur.x, width: cur.width, opacity: 1 });
  }, [activeTabId]);

  useEffect(() => {
    let raf = 0;
    const startedAt = performance.now();
    const tick = (now: number) => {
      placeBar();
      if (now - startedAt < BAR_SETTLE_MS) raf = requestAnimationFrame(tick);
    };
    // 首帧排到下一个动画帧而非同步执行：那是 Flip 真正开始的第一个时刻，
    // 条读到的才是真实位置，而不是一个即将被动画带走的已提交位置。
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
    // 依赖里列 `openTabs`（不只是它的 length）与 `isCompact`：关掉一个**非**激活
    // 标签时 `activeTabId` 不变，但它右侧的所有标签都会左滑；compact 翻转则
    // 一次性重排全部。
  }, [placeBar, openTabs, isCompact]);

  useEffect(() => {
    if (!activeTabId) return;
    const el = stripRef.current?.querySelector<HTMLElement>(`[data-tab-id="${activeTabId}"]`);
    if (el) el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }, [activeTabId, openTabs.length]);

  // 垂直滚轮转横向滚动，与各浏览器标签栏一致。
  useEffect(() => {
    const el = stripRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (e.deltaY === 0 || Math.abs(e.deltaY) < Math.abs(e.deltaX)) return;
      if (el.scrollWidth <= el.clientWidth) return;
      e.preventDefault();
      el.scrollLeft += e.deltaY;
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, []);

  if (descriptors.length === 0) return null;

  return (
    <div
      ref={stripRef}
      role="tablist"
      aria-label="工作区标签"
      className={cn(
        // 这里不用 `border-b`：分隔线是子元素（`below`）而不是边框，因为本条是
        // `overflow-hidden`，它按 *padding* box 裁剪 —— 自带边框会落在裁剪之外，
        // 任何子元素都盖不到，激活标签就无法与下方面板融合。1px 子元素线在裁剪
        // 之内，标签的填充能把它擦掉。
        'relative flex h-9 shrink-0 items-stretch gap-0.5 overflow-hidden bg-card/40 pl-0 pr-1.5',
        // 双保险：标准与 webkit 滚动条都藏。条本身是 `overflow-hidden`，滚动条
        // 其实不会出现在这里 —— 规则留着以防将来有人改回 `overflow-x-auto`。
        'scrollbar-none',
      )}
    >
      {/* 分隔线。`z-0` 让激活标签的填充（z-10）压过底下那条横线。 */}
      <span
        aria-hidden
        className="pointer-events-none absolute inset-x-0 bottom-0 z-0 h-px bg-border"
      />
      {descriptors.map((d) => {
        const active = d.tab.id === activeTabId;
        const dirty = dirtyTabIds.has(d.tab.id);
        return (
          <div
            key={`${d.tab.kind}:${d.tab.id}`}
            data-tab-id={d.tab.id}
            role="tab"
            aria-selected={active}
            onMouseDown={(e) => {
              // Middle-click closes the tab (browser convention).
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
              // 宽度策略（整条内统一）：
              //  - normal：每个标签恰好 `w-[160px]`，条读起来是等宽单元格网格；
              //    标题约容 5–6 个汉字后截断，过长的 hint 在标签内截成 `...`，
              //    而不是把标签撑宽。
              //  - compact（160px 放不下时触发）：每个标签恰好 `w-[92px]`，标题
              //    截到 ≈ 2 个汉字，激活项一眼可辨。hint 隐藏 —— 它的 `shrink-0`
              //    会在视觉上破坏固定的 92px。
              'group relative flex h-full shrink-0 cursor-pointer items-center gap-1.5 rounded-t-md border-x border-t border-transparent px-2.5 text-xs transition-[width,gap,padding] duration-200 ease-out',
              isCompact ? 'w-[92px]' : 'w-[160px]',
              active
                ? 'border-border bg-background text-foreground'
                : // 用 `bg-overlay-hover` 而非手写 `bg-accent/40`：新暗色阶梯上
                  // `--accent`（17.6%）与条自身的底色几乎相同，实心 accent 的
                  // hover 只差约一个亮度级，读起来等于没变。overlay token 是
                  // 一个真实台阶。
                  'text-muted-foreground hover:bg-overlay-hover hover:text-foreground',
              // `z-10` 是激活标签与面板融合的关键：它必须盖过条的分隔线，否则
              // `z-0` 的发丝线子元素会横穿它的底边。
              active && 'z-10',
            )}
            onClick={() => setActiveTab(d.tab.id)}
          >
            {/* 先切肩：切口必须擦掉标签自己的两个下角（填充*与*侧边框），
                所以要盖在它之上、又盖在任何用户看得见的东西之下。 */}
            {active && <TabJoinCorners stripClass="bg-card/40" />}
            <span className={cn('shrink-0', active && d.iconColor)}>{d.icon}</span>
            <span
              // 标题是唯一会伸展的元素，始终截断 —— 但能给它的空间随模式变化：
              //  - normal：~84px（≈ 5 个汉字），hint（如"订单生命周期"）在其右侧。
              //  - compact：~36px（≈ 2 个汉字），hint 隐藏。
              // 完整标题通过 `title` 属性在 hover 时可得。
              //
              // 字重步进挂在标签文字上而不是整个标签：它是唯一字形变化的元素，
              // method pill 与旁边的图标不该跟着加粗。
              className={cn(
                'min-w-0 truncate',
                isCompact ? 'max-w-[36px]' : 'max-w-[88px] flex-1',
                active && 'font-semibold',
              )}
              title={d.kindLabel ? `${d.kindLabel} · ${d.title}` : d.title}
            >
              {d.title}
            </span>
            {/*
             * 尾部控件作为一个 flex item 整体出行，才能用 `ml-auto` 推到标签右缘。
             *
             * 标题带 `flex-1` 但又被 `max-w-[88px]` 封顶，这正是 hint 的空间来源。
             * 该上限也意味着短标题会留下 flexbox 无处安放的空隙 —— 它会落在行的
             * *末端*、关闭按钮之后，于是 `ttt ×` 被挤在 160px 标签的左三分之一、
             * × 后面留一块死白。`ml-auto` 把这段空隙移到这一组控件之前，也就是
             * 浏览器标签放它的位置：标题靠左、× 钉在右。
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
              {d.hint && !isCompact && (
                <Tooltip content={d.hint} side="bottom">
                  <span className="max-w-[40px] truncate text-[10px] text-faint">{d.hint}</span>
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
                    // hover 时始终显示，但激活标签上保持可见。
                    active ? 'opacity-100' : 'opacity-0 group-hover:opacity-100',
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

      {/* 常驻的激活标签指示条。挂在条级以跨标签集变化作为单一元素存活，几何在布局
          运动期间逐帧写入 —— 见上方「Active-tab indicator」。class 里刻意不含
          `w-0`：宽度完全由驱动逐帧写，class 级宽度只会变成与它打架的行内覆盖。 */}
      <span
        ref={barRef}
        aria-hidden
        className="pointer-events-none absolute left-0 top-0 z-20 h-0.5 rounded-t-md bg-primary opacity-0"
      />

      <div className="ml-auto flex items-center pr-1">
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 text-faint"
          onClick={() => {
            // 「关闭其他标签」—— 深潜树后攒下几十个标签时有用。走 dirty 守卫，
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
          onCloseAll={() => {
            // `requestCloseAllTabs` 内部的 dirty 守卫会在任何标签有未保存修改时弹出
            // DirtyCloseDialog，故这里不需要额外的 window.confirm() 提示。
            requestCloseAllTabs();
            setMenu(null);
          }}
          onCloseSaved={() => {
            const n = closeSavedTabs();
            if (n === 0) {
              window.alert('没有可关闭的已保存标签（其余都有未保存的修改）。');
            }
            setMenu(null);
          }}
        />
      )}
    </div>
  );
}
