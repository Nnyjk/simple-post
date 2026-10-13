/**
 * Daemon lifecycle → the tone its hero tile paints with.
 *
 * WHY THIS IS NOT A CONCEPT HUE
 * -----------------------------
 * The MCP hero tile in the connection tab used to carry
 * `concept="connection"`, which is `--hue-connection` — 355°, a rose.
 * A concept hue is an *identity*: "this is the connection tab", the same
 * in every lifecycle, and it is still what the settings rail paints with
 * (see `SettingsPanel.tsx`). But the tile inside the tab sits directly
 * beside a status badge reading `running`, and a rose tile next to a green
 * running badge is not a neutral accent — it reads as an error. The user
 * reported exactly that: the icon stays red while the daemon is serving.
 *
 * So the tile stopped being an identity and became a status readout, and
 * it now has to agree with the badge beside it. The mapping below is a
 * line-for-line mirror of `StatusBadge`'s branches in
 * `src/features/settings/sections/McpStatusPanel.tsx`; if one moves, move
 * the other, or the card contradicts itself again.
 *
 * There is deliberately no colour for `stopped`: a daemon that is simply
 * not running is not an error, and neutral is the honest answer for both
 * that and the pre-first-poll window where we genuinely do not know yet.
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
      // `stopped`, plus `null` (first poll not back) and any string the
      // daemon invents later — the same collapse `StatusBadge` does.
      return 'default';
  }
}
