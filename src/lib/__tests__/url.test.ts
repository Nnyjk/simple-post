/**
 * URL 解析测试 —— `src/lib/url.ts`。
 *
 * 被测行为是 baseUrl 选择器的「自定义」行。`baseUrlOverride` 是三态，只有该键
 * *存在*才选中这个模式：
 *
 *   null / undefined → 走服务链
 *   ''               → 「自定义」但不加前缀：URL 字段就是完整地址
 *   'https://host'   → 「自定义」且带显式主机前缀
 *
 * 中间那档是本文件存在的理由：解析器一旦要求 override *非空*，选「自定义」写入的
 * `''` 在下一次渲染就会被读回成「没有 override」，选择器弹回项目默认，那一行看起来
 * 就像什么都没做。Rust 侧（`resolve_base_url_with`）有同样的收敛，两者必须一致，
 * 否则 cURL 预览与真正发出的请求对 URL 的说法不同。
 *
 * 后一半钉住值层：服务是项目全量列表，某个服务的 URL 是
 * `env.moduleBaseUrls[defId][moduleId]`，格子之间没有回退。
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

/** 项目的服务列表 —— 所有模块共享。 */
const DEFINITIONS: BaseUrlDefinition[] = [
  { id: 'a_data', name: '数据服务', description: '数据面' },
  { id: 'a_auth', name: '鉴权服务', description: '鉴权面' },
  { id: 'b_data', name: '数据服务', description: '数据面' },
  { id: 'b_base', name: '基础服务', description: '基础面' },
];

/**
 * 一个服务，每个模块一个格子。同名的「数据服务」刻意指向两个不同主机——这正是
 * 两层 map 存在的理由，也是扁平的 `baseUrls[defId]` 表达不了的。
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

/** `a_data` 是项目的 ★。 */
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
    // 要点就在这里：环境 base URL 不得偷偷回来。
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

// ---- 值层：(环境 × 模块 × 服务) -----------------------------------------
//
// 每个格子里的值对其他所有格子都是错的，所以项目级值或借来的邻居值都不可能在这里
// 被当成正确答案蒙混过去。

describe('resolveEndpointBaseUrl — one independent cell per (module, service)', () => {
  it('reads the module’s own cell, not another module’s', () => {
    // `a_data` 是两个模块共同的 ★；每个 endpoint 必须落在自己模块的地址上，绝不是
    // 另一个的
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
    // `b_base` 只在 mod_b 下有值。mod_a 必须什么都拿不到，而不是拿到 mod_b 的地址。
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

    // URL 在那个 id 下确实存在，但没有任何东西选中它。
    assert.equal(resolveEndpointBaseUrl(endpoint(), envWithValue, noStar, 'mod_c').baseUrl, '');
    assert.equal(resolveEndpointBaseUrl(endpoint(), envWithValue, noStar, 'mod_c').source, 'none');
  });

  it('resolves nothing for an endpoint that is under no module', () => {
    // `null` 模块没有格子，所以只有「自定义」才可能给出答案。
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
    // `baseUrls[defId]` 是看不见模块的形状。若被读到，endpoint 会解析到一个不属于
    // 任何具体模块的主机——正是两层 map 消除的那种泄漏
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
 * join/替换的先后是有意的——环境自己的 base URL 可能是模板，所以 `{{var}}` 必须在
 * join **之后**解析才能看见它。代价是 `joinBaseUrl` 在还不知道 `{{var}}` 会变成
 * 什么时就插入了分隔符，而值以 `/` 开头的变量会把它变成 `//`：
 * `http://host:9200/{{var}}` → `http://host:9200//api`，对多数服务端那是另一条
 * 路由。`collapseUrlSlashes` 就是替换之后修它的那一遍。
 *
 * 它**不能**越过的边界正是下面这些用例的要点：scheme 的 `//`、protocol-relative
 * 引用、以及 `?` 或 `#` 之后的任何东西都不是 path，而是用户有意输入的数据。
 */
describe('collapseUrlSlashes — the join/substitute order boundary', () => {
  /** 真正的流水线：join → resolve → collapse。 */
  const pipeline = (baseUrl: string, path: string, vars: Record<string, string>) =>
    collapseUrlSlashes(resolveVarsWithMissing(joinBaseUrl(baseUrl, path), vars).value);

  it('repairs the double slash when a variable expands to a leading-slash value', () => {
    // baseUrl 没有尾斜杠，而 path 只有一个标记，所以 `/` 由 join 提供。
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
    // 折叠只去掉*重复*的，绝不去掉 join 贡献的那一个 `/`——否则 `api` 会被粘到主机上
    assert.equal(pipeline('http://host:9200', '{{var}}', { var: 'api' }), 'http://host:9200/api');
  });

  it('leaves the https:// scheme slashes intact', () => {
    assert.equal(collapseUrlSlashes('https://host:9200//api'), 'https://host:9200/api');
    assert.equal(collapseUrlSlashes('https://host:9200/api'), 'https://host:9200/api');
  });

  it('preserves a protocol-relative reference', () => {
    // 无 scheme 的 `//` 是「接下来是主机」的标记。
    assert.equal(collapseUrlSlashes('//cdn.example.com//x'), '//cdn.example.com/x');
  });

  it('leaves an unresolved marker untouched so it still reports as missing', () => {
    // 标记是文本不是分隔符：折叠不能吃掉它，因为 `missingVars` 是用户唯一拿到的信号。
    const joined = joinBaseUrl('http://host:9200', '{{gone}}');
    assert.equal(joined, 'http://host:9200/{{gone}}');
    const r = resolveVarsWithMissing(joined, {});
    const collapsed = collapseUrlSlashes(r.value);
    assert.equal(collapsed, 'http://host:9200/{{gone}}');
    assert.deepEqual(r.missing, ['gone']);
    assert.equal(pipeline('http://host:9200', '{{gone}}', {}), 'http://host:9200/{{gone}}');
  });

  it('does not touch query or fragment data', () => {
    // 查询值里的 `//` 是载荷（嵌套 URL），不是 path。
    assert.equal(
      collapseUrlSlashes('http://host/x?redirect=https://a//b#frag//ment'),
      'http://host/x?redirect=https://a//b#frag//ment',
    );
    // 同一个 URL 的 path 那一侧仍然会折叠。
    assert.equal(collapseUrlSlashes('http://host//x?a=1'), 'http://host/x?a=1');
  });

  it('handles the empty URL without throwing', () => {
    assert.equal(collapseUrlSlashes(''), '');
  });

  it('lets a schemed path win over the base URL, then still collapses it', () => {
    // `joinBaseUrl` 原样返回自带的完整 path；折叠作用于 join 后活下来的那个 path。
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
 * `resolveVarsWithMissing` 让请求层不必手工重扫字符串，就能回答「还有哪些变量没
 * 解析出来」。它必须守住的规则与 Rust 的 `resolve_template` /
 * `list_missing_vars` 一致，因为 MCP daemon 会从另一侧为同一个 endpoint 报同一份
 * 列表。
 */
describe('resolveVarsWithMissing — reports what it could not resolve', () => {
  it('returns an empty missing list when everything resolves', () => {
    const r = resolveVarsWithMissing('/u/{{a}}/{{b}}', { a: '1', b: '2' });
    assert.equal(r.value, '/u/1/2');
    assert.deepEqual(r.missing, []);
  });

  it('keeps the marker in the value AND names it in missing', () => {
    const r = resolveVarsWithMissing('/u/{{a}}/{{gone}}', { a: '1' });
    // 既不清空也不变成 'undefined'——用户必须看见那个洞。
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
    // 判空用 nullish 而非真值：把变量清成 '' 是用户有意的选择，报成「缺失」等于
    // 纠缠一个他故意清空的值
    const r = resolveVarsWithMissing('/u/{{a}}', { a: '' });
    assert.equal(r.value, '/u/');
    assert.deepEqual(r.missing, []);
  });

  it('leaves non-identifier markers alone and does not report them', () => {
    // 标记名只接受 `\w+`，与 Rust 的 `is_word` 一致。`{{ my-var }}` / `{{ a.b }}` 是
    // 字面文本而非变量——报出来只是用户无从处理的噪声
    const r = resolveVarsWithMissing('{{ my-var }} {{ a.b }} {{ok}}', { ok: '1' });
    assert.equal(r.value, '{{ my-var }} {{ a.b }} 1');
    assert.deepEqual(r.missing, []);
  });

  it('is a single non-recursive pass', () => {
    // `{{{{a}}}}` 在下标 2 处含一个格式良好的 `{{a}}`，所以被替换的是它——而*注入
    // 进去*的文本永远不会被再扫一遍。没有这条，值本身是 `{{…}}` 标记的变量会再展开
    // 一次（或死循环）。
    const resolved = resolveVarsWithMissing('{{{{a}}}}', { a: 'x' });
    assert.equal(resolved.value, '{{x}}');
    assert.deepEqual(resolved.missing, []);

    // 同样的形状，未解析：标记原样存活，且只被报一次。
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
