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
 * The listener form's rules, tested without a DOM (the repo has no
 * Vitest / RTL — see AGENTS.md). Each of these guards a mistake that
 * would be silent: a whitelist that silently drops an entry looks like
 * a working allow-list, and a port that silently coerces to 0 asks the
 * OS for a fresh port the user's client config has never heard of.
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
    // A draft that does not round-trip reports "unsaved changes" on a
    // freshly loaded page, which is how users learn to distrust the flag.
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
    // An empty number input means "the user is mid-edit", and silently
    // turning that into 0 would change which port the daemon binds.
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
    // The loopback default must be savable with zero extra input —
    // otherwise the safe state is the one that blocks first-run setup.
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
    // Reporting "no changes" beside an invalid field is how a save
    // silently discards what the user typed.
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
