import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  MCP_CLIENT_PRESETS,
  MCP_SERVER_KEY,
  buildClientConfig,
  buildServerEntry,
  daemonMcpUrl,
  mountPath,
  unavailableReason,
  wrapForClient,
  type McpClientPreset,
  type McpEndpointFacts,
} from '../mcp-client-config';

// 这些测试存在是因为失败模式是**静默**的：客户端拿到错的根键或错的 `type` 值
// 不会报错——服务端只是永远不出现在它的工具列表里。所以一个会发出错误形状的复制按钮
// 比没有复制按钮更糟，而 CI 里没有任何东西会发现。

/** 桌面端：守护进程跑在本机，端口由操作系统分配（配置里写的是 0）。 */
const RUNNING: McpEndpointFacts = {
  shape: 'desktop',
  configPort: 0,
  daemonPort: 51234,
  path: '/mcp',
};

/** Web 端：守护进程跑在服务端进程内部，端口写死在监听配置里，绑定地址是 0.0.0.0。 */
const WEB: McpEndpointFacts = {
  shape: 'web',
  origin: 'http://192.168.12.81:54173',
  configPort: 54174,
  host: '0.0.0.0',
  path: '/mcp',
};

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

describe('mountPath', () => {
  it('adds the leading slash and drops the trailing one', () => {
    assert.equal(mountPath('/mcp'), '/mcp');
    assert.equal(mountPath('mcp'), '/mcp');
    assert.equal(mountPath('/ai/mcp/'), '/ai/mcp');
  });
  it('collapses the root path to an empty segment', () => {
    // MCP 挂在根上时，地址后面不再跟一段路径。
    assert.equal(mountPath('/'), '');
    assert.equal(mountPath(''), '');
  });
});

describe('daemonMcpUrl', () => {
  it('builds the endpoint from the daemon port and the mount path', () => {
    assert.equal(daemonMcpUrl(51234), 'http://127.0.0.1:51234/mcp');
    assert.equal(daemonMcpUrl(51234, '/ai/mcp'), 'http://127.0.0.1:51234/ai/mcp');
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
    assert.equal(
      buildClientConfig(onlyPreset(), {
        shape: 'desktop',
        configPort: 0,
        daemonPort: null,
        path: '/mcp',
      }),
      null,
    );
  });

  it('returns null for a non-positive port rather than guessing', () => {
    assert.equal(
      buildClientConfig(onlyPreset(), {
        shape: 'desktop',
        configPort: 0,
        daemonPort: 0,
        path: '/mcp',
      }),
      null,
    );
  });
});

describe('the desktop form', () => {
  it('uses the configured port instead of the running daemon port', () => {
    // 监听配置里写死了端口时，那个端口就是对外暴露的地址：守护进程重启后客户端配置
    // 不用改。守护进程此刻报的那个端口只是「还没重启」时的旧值。
    const entry = buildServerEntry(onlyPreset(), {
      shape: 'desktop',
      configPort: 54174,
      daemonPort: 51234,
      path: '/mcp',
    });
    assert.ok(entry);
    assert.equal(entry.url, 'http://127.0.0.1:54174/mcp');
  });

  it('falls back to the running daemon port when the configured one is 0', () => {
    const entry = buildServerEntry(onlyPreset(), RUNNING);
    assert.ok(entry);
    assert.equal(entry.url, 'http://127.0.0.1:51234/mcp');
  });

  it('uses the configured mount path', () => {
    const entry = buildServerEntry(onlyPreset(), { ...RUNNING, path: '/ai/mcp' });
    assert.ok(entry);
    assert.equal(entry.url, 'http://127.0.0.1:51234/ai/mcp');
  });
});

describe('unavailableReason', () => {
  it('blames the daemon when no port is known', () => {
    assert.match(
      unavailableReason({
        shape: 'desktop',
        configPort: 0,
        daemonPort: null,
        path: '/mcp',
      }),
      /MCP 未运行/,
    );
  });

  it('never blames the bridge binary — that path is gone', () => {
    // stdio 桥接不在生成的配置形状里，所以指向它的提示只会把用户引去找一个并不需要的安装。
    assert.doesNotMatch(
      unavailableReason({
        shape: 'desktop',
        configPort: 0,
        daemonPort: null,
        path: '/mcp',
      }),
      /桥接/,
    );
  });
});

describe('the web form', () => {
  it('points at the MCP port on the page host, not at the main port', () => {
    // 守护进程跑在服务端进程内部，它的端口要由部署侧发布出来才能被外部客户端连到。
    // 主端口不再提供 MCP，故那里没有可指的地址。
    const entry = buildServerEntry(onlyPreset(), WEB);
    assert.ok(entry);
    assert.equal(entry.url, 'http://192.168.12.81:54174/mcp');
  });

  it('gives no address when no port is fixed in the configuration', () => {
    // 端口写 0 时由系统分配，外部客户端无从知道该连哪一个。
    assert.equal(buildServerEntry(onlyPreset(), { ...WEB, configPort: 0 }), null);
    assert.match(unavailableReason({ ...WEB, configPort: 0 }), /端口/);
  });

  it('gives no address when the bind address is loopback', () => {
    // 守护进程绑在回环地址上时，那个端口只在本机可达，外部客户端连不上。
    assert.equal(buildServerEntry(onlyPreset(), { ...WEB, host: '127.0.0.1' }), null);
    assert.match(unavailableReason({ ...WEB, host: '127.0.0.1' }), /回环/);
  });

  it('uses the configured mount path', () => {
    const entry = buildServerEntry(onlyPreset(), { ...WEB, path: '/ai/mcp' });
    assert.ok(entry);
    assert.equal(entry.url, 'http://192.168.12.81:54174/ai/mcp');
  });

  it('does not blame the daemon when the web endpoint is known', () => {
    assert.doesNotMatch(unavailableReason(WEB), /MCP 未运行/);
  });
});
