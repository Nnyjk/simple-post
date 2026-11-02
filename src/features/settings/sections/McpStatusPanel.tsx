import { useEffect, useRef, useState } from 'react';
import { Loader2, Play, Power, RefreshCw } from 'lucide-react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';
import { useAppStore } from '@/stores/app-store';
import { useMcpStatus } from '@/lib/use-mcp-status';
import type { McpRuntimeStatus, McpUiState } from '@/lib/mcp-types';

// 再导出，好让既有调用方（ConnectionSection、设置标签页等）不必知道面板与共享的
// mcp-types 模块之间的内部切分。
export type { McpRuntimeStatus, McpUiState };

/** 重启 invoke 之后到按钮重新可点之间的宽限期，避免往返抖动时按钮闪成可点。 */
const RESTART_GRACE_MS = 2000;

/**
 * MCP daemon 的重启控件。
 *
 * 真相各归其位：
 *   - 3 秒轮询循环在 Zustand store 里（`useAppStore.startMcpPolling` / `refreshMcpStatus`）。
 *     任何挂载 `useMcpStatus` 的组件都参与同一个单例循环，本面板不再另起 `setInterval`。
 *   - 本面板只留一份 daemon 局部逻辑：`daemon://crashed` 事件监听。Rust 发出该事件时
 *     立即调 `refreshMcpStatus`，徽标当场翻成 `crashed`，不等下一个 3 秒 tick。
 *   - 按钮本身调 `useAppStore.getState().restartMcp()` —— 乐观的「重启中」翻转放在 store，
 *     这样对话框按钮发起的并发重启会呈现同一个可见状态。
 *
 * 本地的 `restarting` 只是观感状态（invoke 在途及宽限期内禁用按钮），**不**遮蔽
 * 徽标所渲染的 daemon 侧 `mcpStatus.status === 'restarting'`。
 */
export function McpStatusPanel() {
  const mcpStatus = useMcpStatus();
  const restartMcp = useAppStore((s) => s.restartMcp);
  const startMcp = useAppStore((s) => s.startMcp);
  const stopMcp = useAppStore((s) => s.stopMcp);

  // 纯观感的本地状态 —— 把「点击 → 禁用」这一下做个去抖，免得抖动往返时闪。
  const [isRestartingLocal, setIsRestartingLocal] = useState(false);
  const [isTogglingLocal, setIsTogglingLocal] = useState(false);
  const graceRef = useRef<number | null>(null);

  // 事件驱动的刷新与宽限计时器清理放在同一个 effect。挂载时订阅一次 `daemon://crashed`，
  // 卸载时退订；监听本身是幂等的 —— Rust 只负责发布，我们调 `refreshMcpStatus` 取新的
  // （崩溃）状态，不必等下一个 3 秒 tick。
  useEffect(() => {
    let cancelled = false;
    const unlistenRefs: UnlistenFn[] = [];

    listen<{ lastError?: string }>('daemon://crashed', (e) => {
      // 立刻刷新，不等下一个 3 秒 tick，并马上把 daemon 给的错误串亮出来，好让
      // 崩溃 / 重启对话框不必等下一次轮询就能显示它。`lastError` 走 slice 的
      // `recordLastError` 动作（不直接 `setState`），渲染层才不会绕过 slice 文档注释
      // 承诺的不变量（尚无快照时为空操作，值没变则短路）。
      const lastError = e.payload?.lastError ?? null;
      void useAppStore
        .getState()
        .refreshMcpStatus()
        .then(() => {
          useAppStore.getState().recordLastError(lastError);
        });
    })
      .then((u) => {
        if (cancelled) {
          u();
          return;
        }
        unlistenRefs.push(u);
      })
      .catch((e) => {
        console.error('[MCP] listen(daemon://crashed) failed', e);
      });

    return () => {
      cancelled = true;
      for (const u of unlistenRefs) u();
      // 清掉待触发的宽限计时器，否则重启途中卸载会把这个已死的实例也翻成可点。
      if (graceRef.current !== null) {
        window.clearTimeout(graceRef.current);
        graceRef.current = null;
      }
    };
  }, []);

  async function handleClick() {
    if (isRestartingLocal) return;
    setIsRestartingLocal(true);
    try {
      await restartMcp();
    } catch {
      // restartMcp 已经记了日志并把徽标翻成 `crashed`。这里只要让按钮重新可点，
      // 用户能在原地重试。
    } finally {
      if (graceRef.current !== null) window.clearTimeout(graceRef.current);
      graceRef.current = window.setTimeout(() => {
        setIsRestartingLocal(false);
        graceRef.current = null;
      }, RESTART_GRACE_MS);
    }
  }

  const status: McpRuntimeStatus = mcpStatus?.status ?? 'stopped';
  const isRunning = status === 'running';
  // 驱动重启图标的危险色 —— daemon 停掉 / 崩溃 / 不健康，正是用户想要「重启」的时刻。
  const isStopped = status === 'stopped' || status === 'crashed' || status === 'unhealthy';
  // 本地宽限期内，或 daemon 自己说正在重启时，禁用按钮。
  const disabled = isRestartingLocal || status === 'restarting' || status === 'starting';
  const toggleDisabled = isTogglingLocal || status === 'restarting' || status === 'starting';

  async function handleToggle() {
    if (isTogglingLocal) return;
    setIsTogglingLocal(true);
    try {
      // 「切换」到底是什么意思由服务端决定：它认为我们在跑就 `stop`，否则 `start`。
      // 拿可能已经过时的轮询状态去猜，就会重启掉本来只想留着的东西。
      if (isRunning) {
        await stopMcp();
      } else {
        await startMcp();
      }
    } catch {
      // 两个动作都已记日志并翻了徽标。恢复可点，让用户能在原地重试。
    } finally {
      if (graceRef.current !== null) window.clearTimeout(graceRef.current);
      graceRef.current = window.setTimeout(() => {
        setIsTogglingLocal(false);
        graceRef.current = null;
      }, RESTART_GRACE_MS);
    }
  }

  return (
    <div className="flex items-center gap-2">
      <Button
        type="button"
        variant={isRunning ? 'outline' : 'secondary'}
        size="sm"
        onClick={() => {
          void handleToggle();
        }}
        disabled={toggleDisabled}
        className="gap-1.5"
      >
        {toggleDisabled ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : isRunning ? (
          <Power className="h-3.5 w-3.5" />
        ) : (
          <Play className="h-3.5 w-3.5" />
        )}
        {toggleDisabled ? '处理中…' : isRunning ? '停止' : '启动'}
      </Button>
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={() => {
          void handleClick();
        }}
        disabled={disabled}
        className="gap-1.5"
      >
        {disabled ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : (
          <RefreshCw className={cn('h-3.5 w-3.5', isStopped && 'text-destructive')} />
        )}
        {disabled ? '重启中…' : '重启 MCP'}
      </Button>
    </div>
  );
}

export function StatusBadge({ status }: { status: McpRuntimeStatus }) {
  if (status === 'running') {
    return (
      <Badge variant="success">
        <span className="mr-1 h-1.5 w-1.5 rounded-full bg-success" />
        running
      </Badge>
    );
  }
  if (status === 'restarting' || status === 'starting') {
    return (
      <Badge variant="warning">
        <Loader2 className="h-3 w-3 animate-spin" />
        restarting…
      </Badge>
    );
  }
  if (status === 'crashed' || status === 'unhealthy') {
    return (
      <Badge variant="destructive">
        <span className="mr-1 h-1.5 w-1.5 rounded-full bg-destructive" />
        {status}
      </Badge>
    );
  }
  return <Badge variant="muted">stopped</Badge>;
}

/** 直接消费 `McpUiState` 快照的精简 `<StateSummary>`。ConnectionSection 传的是 store 传入的
 *  状态。容忍 `null`，对应首次 daemon 轮询完成前的那一小段时间。 */
export function StateSummary({ state }: { state: McpUiState | null }) {
  if (!state) {
    return (
      <span className="inline-flex items-center gap-2 text-muted-foreground">
        <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground" />
        <span className="text-[11px]">连接中…</span>
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-2">
      <StatusBadge status={state.status} />
      {state.pid !== null && (
        <>
          <span className="font-mono text-foreground/80">PID {state.pid}</span>
          <span className="text-muted-foreground/60">·</span>
        </>
      )}
      <span className="text-muted-foreground">
        已处理{' '}
        <span className="font-mono text-foreground">{state.requestsHandled.toLocaleString()}</span>{' '}
        次请求
      </span>
    </span>
  );
}
