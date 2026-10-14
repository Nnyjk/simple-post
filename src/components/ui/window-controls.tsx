/**
 * 自绘窗口控件——只在 Tauri shell（decorations: false）里有效。纯 Vite / 浏览器
 * dev 模式下照样渲染但不起作用，点它们是空操作（由 `safeCall` 里的 catch 兜住）。
 *
 * Win11 式的一组：最小化、最大化/还原、关闭。只有关闭用强调色（与资源管理器、
 * VS Code 以及用户已经信任的每个 Win11 应用一致）。
 *
 * 住在 `components/ui` 而非 `features/top-bar`，因为两个宿主都要它：工作区顶栏，
 * 以及独立欢迎屏——后者是整窗 surface、没有自己的顶栏，否则就既不能最小化也不能
 * 最大化，在 Windows 上还关不掉。
 *
 * Tauri 2 拖拽区注意事项：`data-tauri-drag-region` 那一行里的每个可交互控件都
 * 必须用 `data-tauri-drag-region="false"` 退出，否则 webview 的拖拽手势会吞掉点击。
 */
import { useEffect, useState } from 'react';
import { Minus, Square, Copy, X } from 'lucide-react';
import { Tooltip } from '@/components/ui/tooltip';
import { cn, focusRingBare } from '@/lib/utils';

/**
 * 三个标题栏按钮共用的几何与静止态颜色。hover 处理各不相同（最小化/还原为中性、
 * 关闭为破坏性），所以 hover 类在每个调用点单独传。
 */
const WIN_BTN =
  'flex h-7 w-8 items-center justify-center rounded text-muted-foreground ' +
  'transition-colors hover:bg-accent hover:text-foreground';

/**
 * 包一层，让同一份组件代码能在普通浏览器里跑而不抛异常。在 Tauri shell 内
 * `getCurrentWindow` 的 import 解析到真实后端；在外面，动态 import 加上
 * `window.__TAURI_INTERNALS__` 检查会快速失败并静默空操作。
 */
async function safeCall(fn: () => Promise<void>) {
  if (typeof window === 'undefined') return;
  // 每一个 Tauri 2 webview window 上都有 `__TAURI_INTERNALS__`。这个廉价守卫是为了
  // 让浏览器里的 dev 不会每次点按钮都撞上「ipc not available」。
  const tauriInternals = (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  if (!tauriInternals) return;
  try {
    await fn();
  } catch (e) {
    // 意外错误报到 devtools，但不打断 UI——最小化坏了不该拖垮整个 TopBar。

    console.warn('[WindowControls] tauri call failed:', e);
  }
}

const NO_DRAG = { 'data-tauri-drag-region': 'false' } as const;

export function WindowControls() {
  // 实时跟踪最大化状态，好让中间那个按钮在「最大化」图标（单方块）与「还原」
  // 图标（两个叠放方块）之间切换。订阅 Tauri window 的 `resized` 事件并重新查这个
  // 布尔量——只在点击时查会漏掉双击最大化，以及任何不经过我们的系统级贴靠。
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    if (
      typeof window === 'undefined' ||
      !(window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
    ) {
      return;
    }
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        const win = getCurrentWindow();
        if (cancelled) return;
        setIsMaximized(await win.isMaximized());
        unlisten = await win.onResized(async () => {
          setIsMaximized(await win.isMaximized());
        });
      } catch (e) {
        console.warn('[WindowControls] onResized subscribe failed:', e);
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten) {
        try {
          unlisten();
        } catch {
          // 已退订，无所谓。
        }
      }
    };
  }, []);

  const minimize = () =>
    safeCall(async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().minimize();
    });

  // 显式查最大化状态再派发到对应方法。`toggleMaximize()` 在 API 里是有的，但在某些
  // Tauri 2 构建里是空操作、或与点击处理器竞态——稳妥写法是先读状态再调对应动词。
  const toggleMaximize = () =>
    safeCall(async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const win = getCurrentWindow();
      const maximized = await win.isMaximized();
      if (maximized) {
        await win.unmaximize();
      } else {
        await win.maximize();
      }
    });

  const close = () =>
    safeCall(async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().close();
    });

  return (
    <div className="flex items-center gap-0.5 pl-1">
      <Tooltip content="最小化" side="bottom">
        <button
          {...NO_DRAG}
          type="button"
          onClick={minimize}
          aria-label="最小化"
          className={cn(WIN_BTN, focusRingBare)}
        >
          <Minus className="h-3.5 w-3.5" />
        </button>
      </Tooltip>

      <Tooltip content={isMaximized ? '还原' : '最大化'} side="bottom">
        <button
          {...NO_DRAG}
          type="button"
          onClick={toggleMaximize}
          aria-label={isMaximized ? '还原窗口' : '最大化窗口'}
          className={cn(WIN_BTN, focusRingBare)}
        >
          {/*
            Win11 给这个按钮两种图标形状：
              - 单方块 → 「最大化」（窗口处于普通状态）
              - 两个叠放的方块 → 「还原」（窗口已最大化）
            实时切换，好让这个控件始终与点击将执行的动作一致。
          */}
          {isMaximized ? <Copy className="h-3 w-3" /> : <Square className="h-3 w-3" />}
        </button>
      </Tooltip>

      <Tooltip content="关闭" side="bottom">
        <button
          {...NO_DRAG}
          type="button"
          onClick={close}
          aria-label="关闭窗口"
          className={cn(
            WIN_BTN,
            'hover:bg-destructive hover:text-destructive-foreground',
            focusRingBare,
          )}
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </Tooltip>
    </div>
  );
}
