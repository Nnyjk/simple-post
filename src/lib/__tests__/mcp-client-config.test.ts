import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  MCP_CLIENT_PRESETS,
  MCP_SERVER_KEY,
  buildClientConfig,
  buildServerEntry,
  daemonMcpUrl,
  unavailableReason,
  wrapForClient,
  type McpClientPreset,
} from '../mcp-client-config';

// These tests exist because the failure mode is SILENT. A client given
// the wrong root key or the wrong `type` value does not error — the
// server simply never shows up in its tool list. A copy button that
// ships a wrong shape is therefore worse than no copy button, and
// nothing in CI would ever notice.

const RUNNING = { port: 51234 };

function onlyPreset(): McpClientPreset {
  const [first, ...rest] = MCP_CLIENT_PRESETS;
  assert.ok(first, 'at least one preset must ship');
  assert.deepEqual(rest, [], 'exactly one client format ships');
  return first;
}

function parse(): Record<string, unknown> {
  const json = buildClientConfig(onlyPreset(), RUNNING);
  assert.ok(json, 'the running daemon should generate a snippet');
  return JSON.parse(json);
}

describe('the preset table', () => {
  it('holds exactly one preset, and it is the Claude Code shape', () => {
    // The assertion is the COUNT, not the id. Eight of the nine rows used
    // to sit here, each a hand-maintained transcription of a client's
    // documented format — a table that rots silently, because a stale
    // row still renders and still copies.
    assert.equal(MCP_CLIENT_PRESETS.length, 1);
    assert.equal(MCP_CLIENT_PRESETS[0].id, 'claude-code');
  });

  it('keeps the verification source on the surviving row', () => {
    // Without it the next person cannot tell a checked shape from a
    // guessed one, which is the difference this file exists to protect.
    assert.match(onlyPreset().source, /Claude Code/);
  });

  it('names a config path the user can actually find', () => {
    assert.match(onlyPreset().configPath, /\.mcp\.json/);
  });
});

describe('daemonMcpUrl', () => {
  it('builds the /mcp endpoint from the daemon port', () => {
    assert.equal(daemonMcpUrl(51234), 'http://127.0.0.1:51234/mcp');
  });
  it('returns null when the daemon has no port', () => {
    assert.equal(daemonMcpUrl(null), null);
  });
  it('rejects nonsense ports rather than emitting them into a config', () => {
    assert.equal(daemonMcpUrl(0), null);
    assert.equal(daemonMcpUrl(-1), null);
    assert.equal(daemonMcpUrl(1.5), null);
  });
});

describe('root key', () => {
  it('is mcpServers — the key Claude Code actually reads', () => {
    assert.ok('mcpServers' in parse());
  });

  it('nests the entry under the shared server key', () => {
    const servers = (parse().mcpServers ?? {}) as Record<string, unknown>;
    assert.ok(MCP_SERVER_KEY in servers, `expected a "${MCP_SERVER_KEY}" entry`);
  });

  it('wrapForClient puts the entry under the preset root key', () => {
    assert.deepEqual(wrapForClient(onlyPreset(), { type: 'http' }), {
      mcpServers: { 'simple-post': { type: 'http' } },
    });
  });
});

describe('the `type` field', () => {
  it('is "http" — Claude Code requires it on a remote entry', () => {
    // Omitting `type` here is what the first version of this file did for
    // every client, and no client accepted the result.
    const entry = buildServerEntry(onlyPreset(), RUNNING);
    assert.ok(entry);
    assert.equal(entry.type, 'http');
    assert.equal(entry.url, 'http://127.0.0.1:51234/mcp');
  });

  it('carries no stdio keys, because HTTP needs nothing on the client', () => {
    const entry = buildServerEntry(onlyPreset(), RUNNING);
    assert.ok(entry);
    assert.ok(!('command' in entry), 'no local binary to spawn');
    assert.ok(!('args' in entry), 'no bridge arguments to get wrong');
  });
});

describe('buildClientConfig', () => {
  it('emits valid JSON for the preset it ships', () => {
    const json = buildClientConfig(onlyPreset(), RUNNING);
    assert.ok(json);
    assert.doesNotThrow(() => JSON.parse(json));
  });

  it('names the port the daemon is actually on', () => {
    // The snippet is generated per render from the live snapshot, so it
    // cannot drift onto a port from an earlier session.
    assert.deepEqual(parse(), {
      mcpServers: {
        'simple-post': { type: 'http', url: 'http://127.0.0.1:51234/mcp' },
      },
    });
  });

  it('never emits a literal null or undefined into a snippet', () => {
    const json = buildClientConfig(onlyPreset(), RUNNING);
    assert.ok(json);
    assert.ok(!json.includes('null') && !json.includes('undefined'), json);
  });

  it('never ships an unexpanded %APPDATA% env placeholder', () => {
    const json = buildClientConfig(onlyPreset(), RUNNING);
    assert.ok(json);
    assert.ok(!json.includes('%APPDATA%'), json);
  });

  it('returns null when the daemon is down so the UI can explain why', () => {
    // No port means no URL. Emitting a snippet with a literal `null` in it
    // would be copyable and broken, which is worse than an explanation.
    assert.equal(buildClientConfig(onlyPreset(), { port: null }), null);
  });

  it('returns null for a non-positive port rather than guessing', () => {
    assert.equal(buildClientConfig(onlyPreset(), { port: 0 }), null);
  });
});

describe('unavailableReason', () => {
  it('blames the daemon when no port is known', () => {
    assert.match(unavailableReason(onlyPreset(), { port: null }), /MCP 未运行/);
  });

  it('never blames the bridge binary — that path is gone', () => {
    // The stdio bridge is no longer a generated shape, so a message
    // pointing at it would send the user after an install they do not need.
    assert.doesNotMatch(unavailableReason(onlyPreset(), { port: null }), /桥接/);
  });
});
