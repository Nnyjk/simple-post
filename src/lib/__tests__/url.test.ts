/**
 * URL resolution tests — `src/lib/url.ts`.
 *
 * The behaviour under test is the baseUrl picker's 自定义 row. `baseUrlOverride`
 * is three-state and only the *presence* of the key selects the mode:
 *
 *   null / undefined → follow the service chain
 *   ''               → 自定义, no prefix: the URL field is the whole address
 *   'https://host'   → 自定义 with an explicit host prefix (legacy shape)
 *
 * The middle case is the regression this file exists for. When the
 * resolver required a *non-empty* override, picking 自定义 wrote `''`, the
 * next render read it back as "no override", and the picker snapped back
 * to the project default — the row looked like it did nothing. The same
 * collapse happens in Rust (`resolve_base_url_with`), and the two must
 * agree or the cURL preview and the request that actually goes out
 * disagree about the URL.
 *
 * The second half pins the value layer: services are the project's whole
 * list, the URL for one is `env.moduleBaseUrls[defId][moduleId]`, and
 * there is no fallback between cells.
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import {
  collapseUrlSlashes,
  joinBaseUrl,
  resolveEndpointBaseUrl,
  resolveVars,
  resolveVarsWithMissing,
} from '../url';
import type { BaseUrlDefinition, EndpointDetail, Environment, Project } from '@/types/domain';

/** The project's service list — shared by every module. */
const DEFINITIONS: BaseUrlDefinition[] = [
  { id: 'a_data', name: '数据服务', description: '数据面' },
  { id: 'a_auth', name: '鉴权服务', description: '鉴权面' },
  { id: 'b_data', name: '数据服务', description: '数据面' },
  { id: 'b_base', name: '基础服务', description: '基础面' },
];

/**
 * One service, one cell per module. The same 服务 deliberately points at
 * two different hosts, which is the case the two-level map exists for and
 * the case a flat `baseUrls[defId]` cannot express.
 */
const ENV = {
  name: 'dev',
  variables: {},
  baseUrls: {},
  moduleBaseUrls: {
    a_data: { mod_a: 'https://a-data.example.com', mod_b: 'https://b-data.example.com' },
    a_auth: { mod_a: 'https://a-auth.example.com' },
    b_base: { mod_b: 'https://b-base.example.com' },
  },
} as unknown as Environment;

/** `a_data` is the project ★. */
const PROJECT = {
  baseUrlDefinitions: DEFINITIONS,
  defaultBaseUrlDefinitionId: 'a_data',
} as unknown as Project;

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
    const r = resolveEndpointBaseUrl(endpoint(), ENV, PROJECT, 'mod_a');
    assert.equal(r.baseUrl, 'https://a-data.example.com');
    assert.equal(r.source, 'default');
    assert.equal(r.definitionId, 'a_data');
  });

  it('resolves an explicitly chosen definition over the project default', () => {
    const r = resolveEndpointBaseUrl(
      endpoint({ baseUrlDefinitionId: 'a_auth' }),
      ENV,
      PROJECT,
      'mod_a',
    );
    assert.equal(r.baseUrl, 'https://a-auth.example.com');
    assert.equal(r.source, 'definition');
  });

  it('treats an empty override as 自定义 with no prefix', () => {
    const r = resolveEndpointBaseUrl(endpoint({ baseUrlOverride: '' }), ENV, PROJECT, 'mod_a');
    assert.equal(r.source, 'override');
    // The whole point: no environment base URL sneaks back in.
    assert.equal(r.baseUrl, '');
  });

  it('keeps a legacy host override working as a prefix', () => {
    const r = resolveEndpointBaseUrl(
      endpoint({ baseUrlOverride: 'https://custom.example.com' }),
      ENV,
      PROJECT,
      'mod_a',
    );
    assert.equal(r.source, 'override');
    assert.equal(r.baseUrl, 'https://custom.example.com');
  });

  it('prefers the override even when a definition is also stored', () => {
    const r = resolveEndpointBaseUrl(
      endpoint({ baseUrlDefinitionId: 'a_auth', baseUrlOverride: '' }),
      ENV,
      PROJECT,
      'mod_a',
    );
    assert.equal(r.source, 'override');
    assert.equal(r.baseUrl, '');
  });

  it('resolves to nothing when no env is selected', () => {
    const r = resolveEndpointBaseUrl(endpoint(), undefined, PROJECT, 'mod_a');
    assert.equal(r.baseUrl, '');
    assert.equal(r.source, 'none');
  });
});

// ---- the value layer: (环境 × 模块 × 服务) -------------------------------
//
// Every cell holds the wrong answers for every other cell, so neither a
// project-wide value nor a borrowed neighbour's value can pass here as a
// right one.

describe('resolveEndpointBaseUrl — one independent cell per (module, service)', () => {
  it('reads the module’s own cell, not another module’s', () => {
    // `a_data` is the ★ for both modules; each endpoint must land on its
    // own module's address, never the other's.
    assert.equal(
      resolveEndpointBaseUrl(endpoint(), ENV, PROJECT, 'mod_a').baseUrl,
      'https://a-data.example.com',
    );
    assert.equal(
      resolveEndpointBaseUrl(endpoint(), ENV, PROJECT, 'mod_b').baseUrl,
      'https://b-data.example.com',
    );
  });

  it('lets an explicitly named service resolve in a module the ★ never reached', () => {
    // `b_base` has a value under mod_b only. mod_a must get nothing rather
    // than mod_b's address.
    const inOwnModule = resolveEndpointBaseUrl(
      endpoint({ baseUrlDefinitionId: 'b_base' }),
      ENV,
      PROJECT,
      'mod_b',
    );
    assert.equal(inOwnModule.baseUrl, 'https://b-base.example.com');
    assert.equal(inOwnModule.source, 'definition');

    const elsewhere = resolveEndpointBaseUrl(
      endpoint({ baseUrlDefinitionId: 'b_base' }),
      ENV,
      PROJECT,
      'mod_a',
    );
    assert.equal(elsewhere.baseUrl, '');
    assert.equal(elsewhere.source, 'none');
  });

  it('gives a project with no ★ nothing, with no fallback', () => {
    const noStar = {
      baseUrlDefinitions: [{ id: 'x1', name: '服务' }],
      defaultBaseUrlDefinitionId: null,
    } as unknown as Project;
    const envWithValue = {
      moduleBaseUrls: { x1: { mod_c: 'https://c.example.com' } },
    } as unknown as Environment;

    // A URL exists under the id, but nothing selects it.
    assert.equal(resolveEndpointBaseUrl(endpoint(), envWithValue, noStar, 'mod_c').baseUrl, '');
    assert.equal(resolveEndpointBaseUrl(endpoint(), envWithValue, noStar, 'mod_c').source, 'none');
  });

  it('resolves nothing for an endpoint that is under no module', () => {
    // There is no cell for a `null` module, so only 自定义 could answer.
    const r = resolveEndpointBaseUrl(endpoint(), ENV, PROJECT, null);
    assert.equal(r.baseUrl, '');
    assert.equal(r.source, 'none');
  });

  it('reports "none" when the chosen service has no URL in this env', () => {
    const empty = { moduleBaseUrls: {} } as unknown as Environment;
    const r = resolveEndpointBaseUrl(
      endpoint({ baseUrlDefinitionId: 'a_auth' }),
      empty,
      PROJECT,
      'mod_a',
    );
    assert.equal(r.baseUrl, '');
    assert.equal(r.source, 'none');
  });

  it('never reads the deprecated flat layer', () => {
    // `baseUrls[defId]` is the module-blind shape. If it were consulted, an
    // endpoint would resolve to a host that belongs to no module in
    // particular — the exact leak the two-level map removes.
    const legacy = {
      baseUrls: { b_base: 'https://legacy.example.com' },
      moduleBaseUrls: {},
    } as unknown as Environment;

    const r = resolveEndpointBaseUrl(
      endpoint({ baseUrlDefinitionId: 'b_base' }),
      legacy,
      PROJECT,
      'mod_b',
    );
    assert.equal(r.baseUrl, '');
    assert.equal(r.source, 'none');
  });

  it('refuses an id that names no service of this project', () => {
    const r = resolveEndpointBaseUrl(
      endpoint({ baseUrlDefinitionId: 'not_here' }),
      ENV,
      PROJECT,
      'mod_a',
    );
    assert.equal(r.baseUrl, '');
    assert.equal(r.source, 'none');
    assert.equal(r.definitionId, null);
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

/**
 * The join/substitute ordering is deliberate — the environment's own
 * base URL may be templated, so `{{var}}` has to resolve *after* the
 * join to be able to see it. The price is that `joinBaseUrl` inserts a
 * separator before knowing what `{{var}}` will become, and a variable
 * whose value already starts with `/` turns that separator into a `//`:
 * `http://host:9200/{{var}}` → `http://host:9200//api`, a different
 * route to most servers. `collapseUrlSlashes` is the post-substitution
 * pass that repairs it.
 *
 * The boundaries it must NOT cross are the point of the tests below: the
 * scheme's `//`, a protocol-relative reference, and anything behind `?`
 * or `#` are not path and are data the user typed on purpose.
 */
describe('collapseUrlSlashes — the join/substitute order boundary', () => {
  /** The real pipeline: join → resolve → collapse. */
  const pipeline = (baseUrl: string, path: string, vars: Record<string, string>) =>
    collapseUrlSlashes(resolveVarsWithMissing(joinBaseUrl(baseUrl, path), vars).value);

  it('repairs the double slash when a variable expands to a leading-slash value', () => {
    // The reported bug: baseUrl has no trailing slash and the path is
    // nothing but the marker, so the join supplies the `/`.
    assert.equal(pipeline('http://host:9200', '{{var}}', { var: '/api' }), 'http://host:9200/api');
  });

  it('is idempotent when the baseUrl already ends in a slash', () => {
    assert.equal(pipeline('http://host:9200/', '{{var}}', { var: '/api' }), 'http://host:9200/api');
  });

  it('collapses at the marker boundary when the path continues past it', () => {
    assert.equal(
      pipeline('http://host:9200', '{{var}}/users', { var: '/api' }),
      'http://host:9200/api/users',
    );
  });

  it('keeps the separator when the variable has no leading slash', () => {
    // Collapsing removes *duplicates*, never the single slash that the
    // join contributed — otherwise `api` would be glued onto the host.
    assert.equal(pipeline('http://host:9200', '{{var}}', { var: 'api' }), 'http://host:9200/api');
  });

  it('leaves the https:// scheme slashes intact', () => {
    assert.equal(collapseUrlSlashes('https://host:9200//api'), 'https://host:9200/api');
    assert.equal(collapseUrlSlashes('https://host:9200/api'), 'https://host:9200/api');
  });

  it('preserves a protocol-relative reference', () => {
    // `//` with no scheme is the marker that says "next is a host".
    assert.equal(collapseUrlSlashes('//cdn.example.com//x'), '//cdn.example.com/x');
  });

  it('leaves an unresolved marker untouched so it still reports as missing', () => {
    // The marker is text, not a separator: the collapse must not eat it,
    // because `missingVars` is the only signal the user gets.
    const joined = joinBaseUrl('http://host:9200', '{{gone}}');
    assert.equal(joined, 'http://host:9200/{{gone}}');
    const r = resolveVarsWithMissing(joined, {});
    const collapsed = collapseUrlSlashes(r.value);
    assert.equal(collapsed, 'http://host:9200/{{gone}}');
    assert.deepEqual(r.missing, ['gone']);
    assert.equal(pipeline('http://host:9200', '{{gone}}', {}), 'http://host:9200/{{gone}}');
  });

  it('does not touch query or fragment data', () => {
    // `//` inside a query value is payload (a nested URL), not a path.
    assert.equal(
      collapseUrlSlashes('http://host/x?redirect=https://a//b#frag//ment'),
      'http://host/x?redirect=https://a//b#frag//ment',
    );
    // Still collapses the path side of the same URL.
    assert.equal(collapseUrlSlashes('http://host//x?a=1'), 'http://host/x?a=1');
  });

  it('handles the empty URL without throwing', () => {
    assert.equal(collapseUrlSlashes(''), '');
  });

  it('lets a schemed path win over the base URL, then still collapses it', () => {
    // `joinBaseUrl` returns a self-contained path untouched; the collapse
    // applies to whichever path survived the join.
    assert.equal(
      pipeline('https://custom.example.com', 'https://other/{{var}}', { var: '/api' }),
      'https://other/api',
    );
    assert.equal(
      pipeline('https://custom.example.com', 'https://other/users/1', {}),
      'https://other/users/1',
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
