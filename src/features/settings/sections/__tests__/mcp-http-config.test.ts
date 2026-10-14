import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  DEFAULT_MCP_HTTP_CONFIG,
  configEquals,
  configFromDraft,
  draftError,
  draftFromConfig,
  isDraftDirty,
  isLoopbackHost,
  normalizePath,
  parseList,
  parsePort,
  type McpHttpConfigDraft,
} from '../mcp-http-config-form';
import type { McpHttpConfig } from '@/types/domain';

/**
 * 监听表单的规则，不依赖 DOM 测试（本仓库没有 Vitest / RTL，见 AGENTS.md）。每一条都在
 * 守一个会静默出错的点：悄悄丢掉一条的白名单看着仍像一份能用的允许列表，而悄悄变成 0
 * 的端口等于向系统要一个用户客户端配置里从来没有的新端口。
 */

function draft(overrides: Partial<McpHttpConfigDraft> = {}): McpHttpConfigDraft {
  return {
    host: '127.0.0.1',
    port: '0',
    path: '/mcp',
    allowRemote: false,
    allowedHosts: '',
    allowedOrigins: '',
    ...overrides,
  };
}

const SAVED: McpHttpConfig = DEFAULT_MCP_HTTP_CONFIG;

describe('defaults', () => {
  it('binds loopback, lets the OS pick the port, and stays closed', () => {
    assert.equal(DEFAULT_MCP_HTTP_CONFIG.host, '127.0.0.1');
    assert.equal(DEFAULT_MCP_HTTP_CONFIG.port, 0);
    assert.equal(DEFAULT_MCP_HTTP_CONFIG.path, '/mcp');
    assert.equal(DEFAULT_MCP_HTTP_CONFIG.allowRemote, false);
  });

  it('round-trips a config through the draft form unchanged', () => {
    // 草稿若不能原样往返，刚加载的页面就会报「有未保存修改」，用户于是不再信这个标志。
    assert.deepEqual(configFromDraft(draftFromConfig(SAVED)), SAVED);
  });
});

describe('parsePort', () => {
  it('accepts 0 as "let the OS assign"', () => {
    assert.equal(parsePort('0'), 0);
  });

  it('accepts the top of the range and trims whitespace', () => {
    assert.equal(parsePort(' 65535 '), 65535);
  });

  it('rejects an empty field rather than reading it as 0', () => {
    // 空数字输入框的意思是「用户正在编辑」，悄悄当成 0 会换掉 daemon 绑定的端口。
    assert.equal(parsePort(''), null);
    assert.equal(parsePort('   '), null);
  });

  it('rejects negatives, fractions, over-range and non-numeric text', () => {
    for (const bad of ['-1', '1.5', '65536', 'abc', '0x10', '8080a', '+80']) {
      assert.equal(parsePort(bad), null, `${bad} must not parse as a port`);
    }
  });
});

describe('normalizePath', () => {
  it('adds the leading slash the route needs', () => {
    assert.equal(normalizePath('mcp'), '/mcp');
  });

  it('leaves an already-rooted path alone', () => {
    assert.equal(normalizePath('/mcp'), '/mcp');
  });

  it('falls back to root for an empty field', () => {
    assert.equal(normalizePath('   '), '/');
  });
});

describe('parseList', () => {
  it('splits on newlines', () => {
    assert.deepEqual(parseList('a\nb'), ['a', 'b']);
  });

  it('splits on commas, because that is what a pasted header list looks like', () => {
    assert.deepEqual(parseList('a, b'), ['a', 'b']);
  });

  it('drops blanks and surrounding whitespace', () => {
    assert.deepEqual(parseList('  a  \n\n  b\n   '), ['a', 'b']);
  });

  it('collapses duplicates so re-typing a list does not read as a change', () => {
    assert.deepEqual(parseList('a\na\n a '), ['a']);
  });
});

describe('draftError', () => {
  it('accepts the defaults', () => {
    assert.equal(draftError(draft()), null);
  });

  it('reports an unusable port', () => {
    assert.match(draftError(draft({ port: '70000' })) ?? '', /端口/);
  });

  it('does not require whitelists while remote access is off', () => {
    // 回环默认值必须零输入即可保存 —— 否则最安全的那个状态反倒最挡住首次配置。
    assert.equal(draftError(draft({ allowedHosts: '', allowedOrigins: '' })), null);
  });

  it('requires both whitelists once remote access is on', () => {
    assert.match(draftError(draft({ allowRemote: true })) ?? '', /Host 白名单/);
    assert.match(
      draftError(draft({ allowRemote: true, allowedHosts: 'a' })) ?? '',
      /Origin 白名单/,
    );
  });

  it('demands a scheme on every Origin', () => {
    const e = draftError(
      draft({ allowRemote: true, allowedHosts: 'a', allowedOrigins: 'claude.ai' }),
    );
    assert.match(e ?? '', /协议/);
    assert.match(e ?? '', /https:\/\//, 'the message should show the shape it wants');
  });

  it('accepts a properly schemed Origin', () => {
    assert.equal(
      draftError(
        draft({ allowRemote: true, allowedHosts: 'a', allowedOrigins: 'https://claude.ai' }),
      ),
      null,
    );
  });

  it('treats "*" as the documented no-check escape hatch', () => {
    assert.equal(
      draftError(draft({ allowRemote: true, allowedHosts: '*', allowedOrigins: '*' })),
      null,
    );
  });
});

describe('isLoopbackHost', () => {
  it('recognises the loopback spellings', () => {
    for (const h of ['127.0.0.1', ' 127.0.0.1 ', '::1', 'LOCALHOST']) {
      assert.equal(isLoopbackHost(h), true, `${h} is loopback`);
    }
  });

  it('does not mistake a LAN address for loopback', () => {
    for (const h of ['0.0.0.0', '192.168.1.10', '10.0.0.2']) {
      assert.equal(isLoopbackHost(h), false, `${h} is not loopback`);
    }
  });
});

describe('isDraftDirty', () => {
  it('is false immediately after a hydrate', () => {
    assert.equal(isDraftDirty(draftFromConfig(SAVED), SAVED), false);
  });

  it('is true after any field edit', () => {
    assert.equal(isDraftDirty(draft({ host: '0.0.0.0' }), SAVED), true);
    assert.equal(isDraftDirty(draft({ port: '8080' }), SAVED), true);
    assert.equal(isDraftDirty(draft({ allowRemote: true }), SAVED), true);
  });

  it('ignores whitelist ORDER — a reshuffle is not an edit', () => {
    const saved: McpHttpConfig = {
      ...SAVED,
      allowRemote: true,
      allowedHosts: ['a', 'b'],
      allowedOrigins: ['https://x'],
    };
    const same: McpHttpConfig = { ...saved, allowedHosts: ['b', 'a'] };
    assert.equal(configEquals(saved, same), true);
    assert.equal(isDraftDirty(draftFromConfig(same), saved), false);
  });

  it('is true when the port field cannot be parsed at all', () => {
    // 在一个非法字段旁边显示「无修改」，正是保存悄悄丢掉用户输入的方式。
    assert.equal(isDraftDirty(draft({ port: '' }), SAVED), true);
  });

  it('ignores whitespace-only differences', () => {
    assert.equal(isDraftDirty(draft({ host: '  127.0.0.1  ' }), SAVED), false);
  });
});

describe('configFromDraft', () => {
  it('returns null rather than a config with a bogus port', () => {
    assert.equal(configFromDraft(draft({ port: 'nope' })), null);
  });

  it('normalises a host/path the user typed loosely', () => {
    const c = configFromDraft(draft({ host: '  0.0.0.0 ', path: 'mcp' }));
    assert.equal(c?.host, '0.0.0.0');
    assert.equal(c?.path, '/mcp');
  });

  it('falls back to the loopback default when the host is blanked', () => {
    assert.equal(configFromDraft(draft({ host: '   ' }))?.host, '127.0.0.1');
  });
});
