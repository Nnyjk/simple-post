import { useEffect } from 'react';
import { useAppStore } from '@/stores/app-store';
import type { McpUiState } from '@/lib/mcp-types';

/**
 * 让调用方订阅共享的 MCP daemon 状态，并参与单例轮询的生命周期。
 *
 * 实现契约：
 *   - `startMcpPolling()` 幂等——已有轮询器在跑（例如 `McpStatusPanel` 先挂载）时
 *     调用是空操作。所以 3 秒一拍的节奏在整个渲染器里被去重，全应用只有一个定时器。
 *   - `stopMcpPolling()` 与 effect 清理配对，最后一个卸载的调用方才释放定时器，
 *     与 Zustand 订阅者处理类似共享资源的「消费者投票、最后离场的说了算」一致。
 *   - store 在 `document.visibilityState` 变成 `hidden` 时也会暂停定时器，后台标签页
 *     不会白白地每 3s 唤醒一次 daemon 状态。
 *
 * 返回最新的 `McpUiState`（首次成功轮询之前为 `null`，对应 Tauri 加载型 UI 表示
 * 「还不知道」的习惯写法）。
 */
export function useMcpStatus(): McpUiState | null {
  const status = useAppStore((s) => s.mcpStatus);
  const start = useAppStore((s) => s.startMcpPolling);
  const stop = useAppStore((s) => s.stopMcpPolling);
  useEffect(() => {
    start();
    return () => {
      stop();
    };
  }, [start, stop]);
  return status;
}
