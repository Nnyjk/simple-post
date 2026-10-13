import type { McpHttpConfig } from '@/types/domain';

/**
 * Pure form logic for the Streamable-HTTP listener form.
 *
 * Kept apart from the component on purpose: this file has no React and
 * no IPC, so the parsing rules (whitelist splitting, port bounds, the
 * "remote access needs both whitelists" rule) are unit-testable with
 * nothing but `tsx --test` — the repo has no DOM harness (AGENTS.md).
 * The same split already exists in this folder as `env-utils.tsx`.
 */

/** Server defaults. Mirrors the Rust side so a first paint is never blank. */
export const DEFAULT_MCP_HTTP_CONFIG: McpHttpConfig = {
  host: '127.0.0.1',
  port: 0,
  path: '/mcp',
  allowRemote: false,
  allowedHosts: [],
  allowedOrigins: [],
};

export const MAX_PORT = 65535;

/**
 * The form's working state. Two fields differ from `McpHttpConfig` on
 * purpose:
 *
 * - `port` is a string, because a number input still hands back `''`
 *   mid-edit and holding a number would make clearing it unrepresentable.
 * - the whitelists are raw textarea text, so a half-typed entry is not
 *   thrown away by a round-trip through a parsed array.
 */
export interface McpHttpConfigDraft {
  host: string;
  port: string;
  path: string;
  allowRemote: boolean;
  /** One entry per line; commas are also accepted as separators. */
  allowedHosts: string;
  /** One entry per line; commas are also accepted as separators. */
  allowedOrigins: string;
}

/**
 * Split a whitelist textarea into entries.
 *
 * Newline **and** comma both separate, because both are what people
 * paste (a header list out of a proxy config is comma-joined; the
 * textarea is edited one-per-line). Duplicates collapse so that
 * dirty-detection does not flip on every re-type of the same list.
 */
export function parseList(text: string): string[] {
  const out: string[] = [];
  for (const raw of text.split(/[\n,]/)) {
    const value = raw.trim();
    if (value === '') continue;
    if (!out.includes(value)) out.push(value);
  }
  return out;
}

/** Render a whitelist array back into its one-per-line textarea form. */
export function formatList(list: readonly string[]): string {
  return list.join('\n');
}

/** Seed a draft from the persisted config. */
export function draftFromConfig(config: McpHttpConfig): McpHttpConfigDraft {
  return {
    host: config.host,
    port: String(config.port),
    path: config.path,
    allowRemote: config.allowRemote,
    allowedHosts: formatList(config.allowedHosts),
    allowedOrigins: formatList(config.allowedOrigins),
  };
}

/**
 * Parse the port field. `null` means "not a usable port" and is what
 * the validator reports on.
 *
 * The regex rejects more than `Number()` would: `'-1'`, `'1.5'`,
 * `'0x10'` and `' 12 '`-with-inner-junk all survive `Number()` in some
 * form, and an OS-assigned port is the only non-negative value we want.
 */
export function parsePort(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const value = Number(trimmed);
  if (!Number.isInteger(value) || value < 0 || value > MAX_PORT) return null;
  return value;
}

/** Force a leading slash and fall back to `/` for an empty field. */
export function normalizePath(text: string): string {
  const trimmed = text.trim();
  if (trimmed === '') return '/';
  return trimmed.startsWith('/') ? trimmed : `/${trimmed}`;
}

/** Draft → wire config, or `null` when the port field is unusable. */
export function configFromDraft(draft: McpHttpConfigDraft): McpHttpConfig | null {
  const port = parsePort(draft.port);
  if (port === null) return null;
  return {
    host: draft.host.trim() === '' ? DEFAULT_MCP_HTTP_CONFIG.host : draft.host.trim(),
    port,
    path: normalizePath(draft.path),
    allowRemote: draft.allowRemote,
    allowedHosts: parseList(draft.allowedHosts),
    allowedOrigins: parseList(draft.allowedOrigins),
  };
}

/** Field-by-field equality — order-insensitive on the whitelists. */
export function configEquals(a: McpHttpConfig, b: McpHttpConfig): boolean {
  const same = (x: readonly string[], y: readonly string[]) => {
    const sx = [...x].sort();
    const sy = [...y].sort();
    return sx.length === sy.length && sx.every((v, i) => v === sy[i]);
  };
  return (
    a.host === b.host &&
    a.port === b.port &&
    a.path === b.path &&
    a.allowRemote === b.allowRemote &&
    same(a.allowedHosts, b.allowedHosts) &&
    same(a.allowedOrigins, b.allowedOrigins)
  );
}

/**
 * Unsaved-changes flag.
 *
 * An unparseable draft counts as dirty rather than as "clean": the user
 * typed something, and reporting "no changes" next to a field the form
 * calls invalid is how a save silently drops their edit.
 */
export function isDraftDirty(draft: McpHttpConfigDraft, saved: McpHttpConfig): boolean {
  const next = configFromDraft(draft);
  if (next === null) return true;
  return !configEquals(next, saved);
}

/** `localhost` / `127.0.0.1` / `::1` — the addresses that need no whitelist. */
export function isLoopbackHost(host: string): boolean {
  const value = host.trim().toLowerCase();
  return value === '127.0.0.1' || value === '::1' || value === 'localhost';
}

/** Does this Origin entry carry a scheme? `https://…` does, `example.com` does not. */
function hasScheme(origin: string): boolean {
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(origin);
}

/**
 * Blocking problem with the current draft, or `null` when it can be
 * saved. Ordered so the message names the field the user is looking at.
 */
export function draftError(draft: McpHttpConfigDraft): string | null {
  if (parsePort(draft.port) === null) {
    return `端口需为 0–${MAX_PORT} 的整数，0 表示由系统分配`;
  }
  if (draft.allowRemote) {
    const hosts = parseList(draft.allowedHosts);
    const origins = parseList(draft.allowedOrigins);
    if (hosts.length === 0) return '开启远程访问后，至少需要一个 Host 白名单条目';
    if (origins.length === 0) return '开启远程访问后，至少需要一个 Origin 白名单条目';
    // `*` is the documented "no check" escape hatch, so it is exempt.
    if (!hosts.includes('*') && !origins.includes('*')) {
      const bare = origins.filter((o) => !hasScheme(o));
      if (bare.length > 0) return `Origin 必须带协议，例如 https://${bare[0]}`;
    }
  }
  return null;
}
