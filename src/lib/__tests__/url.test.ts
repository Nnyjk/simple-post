/**
 * URL resolution tests — `src/lib/url.ts`.
 *
 * The behaviour under test is the baseUrl picker's 自定义 row. `baseUrlOverride`
 * is three-state and only the *presence* of the key selects the mode:
 *
 *   null / undefined → follow the project's definition chain
 *   ''               → 自定义, no prefix: the URL field is the whole address
 *   'https://host'   → 自定义 with an explicit host prefix (legacy shape)
 *
 * The middle case is the regression this file exists for. When the
 * resolver required a *non-empty* override, picking 自定义 wrote `''`, the
 * next render read it back as "no override", and the picker snapped back to
 * 项目默认 — the row looked like it did nothing. The same collapse happens
 * in Rust (`resolve_base_url_with`), and the two must agree or the cURL
 * preview and the request that actually goes out disagree about the URL.
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { joinBaseUrl, resolveEndpointBaseUrl, resolveVars, resolveVarsWithMissing } from '../url';
import type { BaseUrlDefinition, EndpointDetail, Environment, Project } from '@/types/domain';

const DEFINITIONS: BaseUrlDefinition[] = [
  { id: 'data', name: '数据服务', description: '数据面' },
  { id: 'auth', name: '鉴权服务', description: '鉴权面' },
];

const PROJECT = {
  baseUrlDefinitions: DEFINITIONS,
  defaultBaseUrlDefinitionId: 'data',
} as unknown as Project;

const ENV = {
  name: 'dev',
  variables: {},
  baseUrls: {
    data: 'https://data.example.com',
    auth: 'https://auth.example.com',
  },
} as unknown as Environment;

function endpoint(over: Partial<EndpointDetail> = {}): EndpointDetail {
  return {
    environmentId: ENV.name,
    baseUrlDefinitionId: null,
    baseUrlOverride: null,
    ...over,
  } as unknown as EndpointDetail;
}

describe('resolveEndpointBaseUrl — 自定义 is selected by presence, not truthiness', () => {
  it('follows the project default when the override is null', () => {
    const r = resolveEndpointBaseUrl(endpoint(), ENV, PROJECT);
    assert.equal(r.baseUrl, 'https://data.example.com');
    assert.equal(r.source, 'default');
    assert.equal(r.definitionId, 'data');
  });

  it('resolves an explicitly chosen definition over the project default', () => {
    const r = resolveEndpointBaseUrl(endpoint({ baseUrlDefinitionId: 'auth' }), ENV, PROJECT);
    assert.equal(r.baseUrl, 'https://auth.example.com');
    assert.equal(r.source, 'definition');
  });

  it('treats an empty override as 自定义 with no prefix', () => {
    const r = resolveEndpointBaseUrl(endpoint({ baseUrlOverride: '' }), ENV, PROJECT);
    assert.equal(r.source, 'override');
    // The whole point: no environment base URL sneaks back in.
    assert.equal(r.baseUrl, '');
  });

  it('keeps a legacy host override working as a prefix', () => {
    const r = resolveEndpointBaseUrl(
      endpoint({ baseUrlOverride: 'https://custom.example.com' }),
      ENV,
      PROJECT,
    );
    assert.equal(r.source, 'override');
    assert.equal(r.baseUrl, 'https://custom.example.com');
  });

  it('prefers the override even when a definition is also stored', () => {
    const r = resolveEndpointBaseUrl(
      endpoint({ baseUrlDefinitionId: 'auth', baseUrlOverride: '' }),
      ENV,
      PROJECT,
    );
    assert.equal(r.source, 'override');
    assert.equal(r.baseUrl, '');
  });

  it('resolves to nothing when no env is selected', () => {
    const r = resolveEndpointBaseUrl(endpoint(), undefined, PROJECT);
    assert.equal(r.baseUrl, '');
    assert.equal(r.source, 'none');
  });
});

describe('joinBaseUrl — a 自定义 URL field is never prefixed', () => {
  it('leaves a full URL alone in custom mode (no base to join)', () => {
    assert.equal(joinBaseUrl('', 'https://api.other.com/users/1'), 'https://api.other.com/users/1');
  });

  it('leaves a relative path alone when there is no base', () => {
    assert.equal(joinBaseUrl('', '/users/1'), '/users/1');
  });

  it('still lets a schemed path beat a legacy host override', () => {
    assert.equal(
      joinBaseUrl('https://custom.example.com', 'https://api.other.com/users/1'),
      'https://api.other.com/users/1',
    );
  });

  it('joins path onto a resolved environment base URL', () => {
    assert.equal(
      joinBaseUrl('https://data.example.com', '/users/1'),
      'https://data.example.com/users/1',
    );
    assert.equal(
      joinBaseUrl('https://data.example.com/', 'users/1'),
      'https://data.example.com/users/1',
    );
  });
});

describe('resolveVars — 自定义 does not disable environment variables', () => {
  it('substitutes {{var}} in a full custom URL', () => {
    const env = { variables: { token: 'abc123' } } as unknown as Environment;
    assert.equal(
      resolveVars('https://api.example.com/u/{{token}}', env.variables),
      'https://api.example.com/u/abc123',
    );
  });

  it('leaves unknown variables intact so the preview shows what is missing', () => {
    assert.equal(resolveVars('/u/{{nope}}', { token: 'abc' }), '/u/{{nope}}');
  });
});

/**
 * `resolveVarsWithMissing` is what lets the request layer answer "which
 * variables are still unresolved?" without re-scanning strings by hand.
 * The rules it must keep are the ones Rust's `resolve_template` /
 * `list_missing_vars` enforce, because the MCP daemon reports the same
 * list for the same endpoint from the other side.
 */
describe('resolveVarsWithMissing — reports what it could not resolve', () => {
  it('returns an empty missing list when everything resolves', () => {
    const r = resolveVarsWithMissing('/u/{{a}}/{{b}}', { a: '1', b: '2' });
    assert.equal(r.value, '/u/1/2');
    assert.deepEqual(r.missing, []);
  });

  it('keeps the marker in the value AND names it in missing', () => {
    const r = resolveVarsWithMissing('/u/{{a}}/{{gone}}', { a: '1' });
    // Not blanked, not 'undefined' — the user must see the hole.
    assert.equal(r.value, '/u/1/{{gone}}');
    assert.deepEqual(r.missing, ['gone']);
  });

  it('dedupes by first occurrence and preserves order', () => {
    const r = resolveVarsWithMissing('{{z}}-{{a}}-{{z}}-{{a}}', {});
    assert.deepEqual(r.missing, ['z', 'a']);
    assert.equal(r.value, '{{z}}-{{a}}-{{z}}-{{a}}');
  });

  it('accepts both an Environment and a raw record', () => {
    const env = { variables: { a: '1' }, baseUrls: {} } as unknown as Environment;
    assert.deepEqual(resolveVarsWithMissing('{{a}}{{gone}}', env).missing, ['gone']);
    assert.deepEqual(resolveVarsWithMissing('{{a}}{{gone}}', { a: '1' }).missing, ['gone']);
  });

  it('treats a variable defined as the empty string as resolved, not missing', () => {
    // Nullish, not truthy: clearing a variable to '' is a deliberate
    // choice, and reporting it as "missing" would nag about a value the
    // user emptied on purpose.
    const r = resolveVarsWithMissing('/u/{{a}}', { a: '' });
    assert.equal(r.value, '/u/');
    assert.deepEqual(r.missing, []);
  });

  it('leaves non-identifier markers alone and does not report them', () => {
    // The marker name is `\w+` only, matching Rust's `is_word`.
    // `{{ my-var }}` / `{{ a.b }}` are literal text, not variables —
    // reporting them would be noise the user cannot act on.
    const r = resolveVarsWithMissing('{{ my-var }} {{ a.b }} {{ok}}', { ok: '1' });
    assert.equal(r.value, '{{ my-var }} {{ a.b }} 1');
    assert.deepEqual(r.missing, []);
  });

  it('is a single non-recursive pass', () => {
    // `{{{{a}}}}` contains a well-formed `{{a}}` starting at index 2,
    // so that is what gets replaced — and the *injected* text is never
    // rescanned. Without that, a variable whose value is itself a
    // `{{…}}` marker would expand again (or loop).
    const resolved = resolveVarsWithMissing('{{{{a}}}}', { a: 'x' });
    assert.equal(resolved.value, '{{x}}');
    assert.deepEqual(resolved.missing, []);

    // Same shape, unresolved: the marker survives untouched and is
    // reported exactly once.
    const missing = resolveVarsWithMissing('{{{{gone}}}}', {});
    assert.equal(missing.value, '{{{{gone}}}}');
    assert.deepEqual(missing.missing, ['gone']);
  });

  it('handles the empty input without throwing', () => {
    assert.deepEqual(resolveVarsWithMissing('', { a: '1' }), { value: '', missing: [] });
  });

  it('resolveVars is the same implementation, minus the list', () => {
    const input = '/u/{{a}}/{{gone}}';
    const vars = { a: '1' };
    assert.equal(resolveVars(input, vars), resolveVarsWithMissing(input, vars).value);
  });
});
