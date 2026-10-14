/**
 * daemon 生命周期 → 它的 hero tile 该涂的色调。
 *
 * 为什么这不是概念色相
 * -------------------
 * 概念色相表达的是*身份*（「这是连接页」，每种生命周期都一样），设置侧栏至今仍用它上色
 * （见 `SettingsPanel.tsx`）。但页签里的 tile 紧挨着一个写着 `running` 的状态徽标，
 * 玫红 tile 配绿色徽标不是中性强调——它读起来就是报错。
 *
 * 所以 tile 不再是身份而是状态读数，必须与旁边的徽标一致。下面的映射与
 * `src/features/settings/sections/McpStatusPanel.tsx` 里 `StatusBadge` 的分支逐行
 * 对应：动一个就得动另一个，否则卡片自相矛盾。
 *
 * `stopped` 刻意没有颜色：只是没在跑的 daemon 不是错误，而中性色对「确实还没在跑」
 * 以及首次轮询前的未知窗口都是诚实的答案。
 */
import type { McpRuntimeStatus } from './mcp-types';

export type DaemonTone = 'default' | 'success' | 'warning' | 'danger';

export function daemonTone(status: McpRuntimeStatus | null | undefined): DaemonTone {
  switch (status) {
    case 'running':
      return 'success';
    case 'starting':
    case 'restarting':
      return 'warning';
    case 'crashed':
    case 'unhealthy':
      return 'danger';
    default:
      // `stopped`，加上 `null`（首次轮询未回）与 daemon 日后自创的任何字符串——
      // 与 `StatusBadge` 一样的收敛
      return 'default';
  }
}
