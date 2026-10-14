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

// 这些测试存在是因为失败模式是**静默**的：客户端拿到错的根键或错的 `type` 值
// 不会报错——服务端只是永远不出现在它的工具列表里。所以一个会发出错误形状的复制按钮
// 比没有复制按钮更糟，而 CI 里没有任何东西会发现。

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
    // 断言的是**数量**而不是 id。每一行都是某个客户端文档格式的手抄——过期的行照样
    // 渲染、照样能复制，所以这张表会静默过期。
    assert.equal(MCP_CLIENT_PRESETS.length, 1);
    assert.equal(MCP_CLIENT_PRESETS[0].id, 'claude-code');
  });

  it('keeps the verification source on the surviving row', () => {
    // 没有它，下一个改动的人就分不清这个形状是查证过的还是猜的，而这正是本文件要守的。
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
    // 漏掉 `type` 就是错的：Claude Code 的远程项要求它。
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
    // 片段每次渲染都由实时快照生成，所以不会漂到上一次会话的端口上。
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
    // 没有端口就没有 URL。发出一个里面带着字面量 `null` 的片段会「能复制但坏掉」，
    // 那比一句解释更糟。
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
    // stdio 桥接不再是生成的形状，所以指向它的提示只会把用户引去找一个并不需要的安装。
    assert.doesNotMatch(unavailableReason(onlyPreset(), { port: null }), /桥接/);
  });
});
