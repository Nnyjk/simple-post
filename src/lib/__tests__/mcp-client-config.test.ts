import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  MCP_CLIENT_PRESETS,
  buildClientConfig,
  buildServerEntry,
  bridgeTargetUrl,
  daemonMcpUrl,
  unavailableReason,
  type McpClientPreset,
  type McpTransport,
} from '../mcp-client-config';

// These tests exist because the failure mode is SILENT. A client given
// the wrong root key or the wrong `type` value does not error — the
// server simply never shows up in its tool list. A copy button that
// ships a wrong shape is therefore worse than no copy button, and
// nothing in CI would ever notice. So each documented quirk is pinned.

const RUNNING = { port: 51234, proxyPath: 'C:\\apps\\simple-post-mcp.exe' };

function preset(id: string): McpClientPreset {
  const found = MCP_CLIENT_PRESETS.find((p) => p.id === id);
  assert.ok(found, `preset ${id} should exist`);
  return found;
}

function parse(presetId: string, t: McpTransport): Record<string, unknown> {
  const json = buildClientConfig(preset(presetId), t, RUNNING);
  assert.ok(json, `${presetId}/${t} should generate`);
  return JSON.parse(json);
}

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

describe('bridgeTargetUrl', () => {
  it('points at the daemon ROOT, not /mcp', () => {
    // `simple-post-mcp --url=` appends `/mcp` itself.
    assert.equal(bridgeTargetUrl(51234), 'http://127.0.0.1:51234');
    assert.ok(!bridgeTargetUrl(51234)?.endsWith('/mcp'));
  });
});

describe('root key', () => {
  it('is mcpServers for the Claude family, Cursor, Cline, Windsurf, Continue', () => {
    for (const id of ['claude-desktop', 'claude-code', 'cursor', 'cline', 'windsurf', 'continue']) {
      const obj = parse(id, 'stdio');
      assert.ok('mcpServers' in obj, `${id} should use mcpServers, got ${Object.keys(obj)}`);
    }
  });

  it('is servers for VS Code — mcpServers there is silently ignored', () => {
    const obj = parse('vscode', 'stdio');
    assert.ok('servers' in obj, 'VS Code reads `servers`');
    assert.ok(!('mcpServers' in obj), 'mcpServers would be silently ignored by VS Code');
  });

  it('is context_servers for Zed', () => {
    const obj = parse('zed', 'stdio');
    assert.ok('context_servers' in obj);
    assert.ok(!('mcpServers' in obj));
  });
});

describe('the `type` field', () => {
  it('is REQUIRED on Cursor stdio (documented as Yes in their field table)', () => {
    const entry = buildServerEntry(preset('cursor'), 'stdio', RUNNING);
    assert.equal(entry?.type, 'stdio');
  });

  it('is absent on Cursor remote — Cursor infers transport from `url`', () => {
    const entry = buildServerEntry(preset('cursor'), 'http', RUNNING);
    assert.ok(entry);
    assert.equal('type' in entry, false, 'the documented remote example carries only url');
    assert.equal(entry.url, 'http://127.0.0.1:51234/mcp');
  });

  it('is "http" on the Claude family and VS Code', () => {
    for (const id of ['claude-desktop', 'claude-code', 'vscode', 'http-generic']) {
      const entry = buildServerEntry(preset(id), 'http', RUNNING);
      assert.equal(entry?.type, 'http', `${id} should emit type: "http"`);
    }
  });

  it('is "streamableHttp" on Cline — "http" would silently fall back to SSE', () => {
    const entry = buildServerEntry(preset('cline'), 'http', RUNNING);
    // The literal itself is the assertion: Cline treats anything else
    // (including "http", and especially a missing `type`) as the legacy
    // HTTP+SSE transport and then fails to connect.
    assert.equal(entry?.type, 'streamableHttp');
    assert.equal(entry?.url, 'http://127.0.0.1:51234/mcp');
  });

  it('is absent on Zed — it infers from command vs url and rejects a type field', () => {
    for (const t of ['stdio', 'http'] as const) {
      const entry = buildServerEntry(preset('zed'), t, RUNNING);
      assert.ok(entry);
      assert.equal('type' in entry, false, `Zed must not receive a type field (${t})`);
    }
  });

  it('is absent on VS Code stdio — their own example omits it', () => {
    const entry = buildServerEntry(preset('vscode'), 'stdio', RUNNING);
    assert.ok(entry);
    assert.equal('type' in entry, false);
    assert.equal(entry.command, RUNNING.proxyPath);
  });
});

describe('Zed specifics', () => {
  it('carries source:"custom" and env on stdio, and no type', () => {
    const obj = parse('zed', 'stdio') as {
      context_servers: Record<string, Record<string, unknown>>;
    };
    const e = obj.context_servers['simple-post'];
    assert.equal(e.source, 'custom');
    assert.ok(e.env && typeof e.env === 'object', 'Zed expects an env object');
    assert.deepEqual(e.args, ['--url', 'http://127.0.0.1:51234']);
  });

  it('carries source:"custom" on remote too', () => {
    const e = buildServerEntry(preset('zed'), 'http', RUNNING);
    assert.equal(e?.source, 'custom');
    assert.equal(e?.url, 'http://127.0.0.1:51234/mcp');
  });
});

describe('stdio command shape', () => {
  it('never passes a bare "stdio" positional arg', () => {
    // `simple-post-mcp` takes --transport=stdio as a FLAG; a positional
    // arg makes clap exit 2 with "unexpected argument 'stdio' found".
    const entry = buildServerEntry(preset('claude-desktop'), 'stdio', RUNNING);
    assert.ok(entry);
    // Check `args`, not the whole entry: `"type": "stdio"` is a
    // legitimate transport marker, a bare `"stdio"` POSITIONAL arg is
    // what makes clap exit 2.
    assert.ok(
      Array.isArray(entry.args) && !entry.args.includes('stdio'),
      'a bare "stdio" positional arg makes the bridge fail to parse',
    );
    assert.deepEqual(entry.args, ['--url', 'http://127.0.0.1:51234']);
  });

  it('uses an ABSOLUTE command path so spawn works without PATH', () => {
    for (const id of ['claude-desktop', 'cursor', 'cline', 'vscode', 'zed']) {
      const entry = buildServerEntry(preset(id), 'stdio', RUNNING);
      const cmd = String(entry?.command);
      assert.ok(
        cmd.includes('/') || cmd.includes('\\'),
        `${id} command must be an absolute path, got: ${cmd}`,
      );
    }
  });
});

describe('buildClientConfig', () => {
  it('emits valid JSON for every preset and transport it claims to support', () => {
    for (const p of MCP_CLIENT_PRESETS) {
      for (const t of p.supports) {
        const json = buildClientConfig(p, t, RUNNING);
        assert.ok(json, `${p.id}/${t} should generate`);
        assert.doesNotThrow(() => JSON.parse(json), `${p.id}/${t} must be valid JSON`);
      }
    }
  });

  it('never emits a literal null or undefined into a snippet', () => {
    for (const p of MCP_CLIENT_PRESETS) {
      for (const t of p.supports) {
        const json = buildClientConfig(p, t, RUNNING);
        assert.ok(json);
        assert.ok(
          !json.includes('null') && !json.includes('undefined'),
          `${p.id}/${t} leaked a nullish value: ${json}`,
        );
      }
    }
  });

  it('never ships an unexpanded %APPDATA% env placeholder', () => {
    // env values go to the child verbatim; there is no shell to expand.
    for (const p of MCP_CLIENT_PRESETS) {
      for (const t of p.supports) {
        const json = buildClientConfig(p, t, RUNNING);
        assert.ok(json);
        assert.ok(!json.includes('%APPDATA%'), `${p.id}/${t} leaked %APPDATA%`);
      }
    }
  });

  it('produces an http config even when the bridge path is unresolved', () => {
    // HTTP needs no binary, so a missing bridge path must not disable it.
    for (const p of MCP_CLIENT_PRESETS) {
      if (!p.supports.includes('http')) continue;
      const json = buildClientConfig(p, 'http', { port: 51234, proxyPath: null });
      assert.ok(json, `${p.id} http should not need proxyPath`);
    }
  });

  it('returns null when the daemon is down so the UI can explain why', () => {
    assert.equal(
      buildClientConfig(preset('cursor'), 'http', { port: null, proxyPath: null }),
      null,
    );
  });
});

describe('unavailableReason', () => {
  it('blames the daemon first when no port is known', () => {
    assert.match(
      unavailableReason(preset('cursor'), 'http', { port: null, proxyPath: null }),
      /MCP 未运行/,
    );
  });

  it('blames the bridge path when only stdio is affected', () => {
    assert.match(
      unavailableReason(preset('cursor'), 'stdio', { port: 51234, proxyPath: null }),
      /桥接二进制/,
    );
    assert.doesNotMatch(
      unavailableReason(preset('cursor'), 'http', { port: 51234, proxyPath: null }),
      /桥接二进制/,
    );
  });

  it('names an unsupported transport instead of blaming the environment', () => {
    const r = unavailableReason(preset('continue'), 'http', RUNNING);
    assert.match(r, /未提供/);
  });
});
