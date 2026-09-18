import { useEffect } from 'react';
import { useAppStore } from '@/stores/app-store';
import type { McpUiState } from '@/lib/mcp-types';

/**
 * Subscribe the calling component to the shared MCP daemon status and
 * participate in the singleton polling lifecycle.
 *
 * Implementation contract:
 *   - `startMcpPolling()` is idempotent — if a poller is already running
 *     (because another component — e.g. `McpStatusPanel` — mounted
 *     first) the call is a no-op. The 3-second tick is therefore
 *     deduplicated across the whole renderer, eliminating the duplicate
 *     `setInterval` that previously ran in both `TopBar` and
 *     `McpStatusPanel`.
 *   - `stopMcpPolling()` is paired with the effect cleanup so the
 *     last-unmounted caller releases the timer. This matches the
 *     "consumers vote, last one wins" style Zustand-subscribers use
 *     for similar shared resources (see `useMcpStatus` callers).
 *   - The store also pauses the timer when `document.visibilityState`
 *     becomes `hidden`, so a backgrounded tab doesn't wake up the
 *     daemon state every 3s for nothing.
 *
 * Returns the latest `McpUiState` (or `null` before the first
 * successful poll, mirroring how Tauri-loaded UI typically signals
 * "not yet known").
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
