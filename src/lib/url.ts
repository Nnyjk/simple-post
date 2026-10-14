/**
 * URL 辅助 —— 纯函数、无框架依赖。HttpTester、BaseUrlPicker 与 cURL 导出共用
 * 同一套 join / 解析逻辑；要改解析规则就只在这里改一次。
 */
import type { BaseUrlDefinition, EndpointDetail, Environment, Project } from '@/types/domain';

/** 生效的 baseUrl 来自哪一行；选择器用它标注当前选择（"项目默认服务" /
 *  "鉴权服务" / "自定义"）。`default` 是项目 ★，即
 *  `Project.defaultBaseUrlDefinitionId`，因 endpoint 自身没指定服务而命中。 */
export type BaseUrlSource = 'override' | 'definition' | 'default' | 'none';

export interface ResolvedBaseUrl {
  baseUrl: string;
  source: BaseUrlSource;
  /** 贡献该值的 baseUrl definition 的 id，无则 `null`。 */
  definitionId: string | null;
  /** 贡献该值的 definition 名。 */
  definitionName: string | null;
}

/**
 * 解析 endpoint 生效的 baseUrl。解析顺序（优先级由高到低）：
 *   1. `endpoint.baseUrlOverride`      —— 每 endpoint 的「自定义」模式
 *   2. `endpoint.baseUrlDefinitionId`  —— endpoint 命名的服务，未指定则
 *      取 `Project.defaultBaseUrlDefinitionId`
 *   3. `env.moduleBaseUrls[defId][moduleId]` —— 唯一的值层
 *   4. `''`                             —— 什么都没有
 *
 * override 是三态，「有值但为空」是有意义的：
 *   `null` / `undefined` —— 不在自定义模式，继续走服务链
 *   `''`                 —— 自定义但不加前缀：URL 字段本身就是完整地址
 *   `'https://x'`        —— 自定义且带显式主机前缀
 *
 * 这正是 `double_option` patch 约定在干活：调用方写 `baseUrlOverride: ''`
 * 是**记录这个选择**而不是清空，只有 `null` 才清空。把 `''` 并回服务链，
 * 选择器里的「自定义」行看起来就像什么都没做。
 *
 * 服务是项目级的，值是 (环境 × 模块 × 服务) 的：`project.baseUrlDefinitions`
 * 是项目全量列表，所有模块都看得见，指名服务永远不是跨模块错误；模块相关的是
 * 取值的那个格子，所以 `moduleId` 是必填参数——同一服务在两个模块指向两个
 * 主机，正是这个模型要表达的。
 *
 * **没有回退层。** `baseUrls` 已废弃且从不读取；空格子给出 `''` / `'none'`，
 * 绝不借用别的模块或别的服务的 URL。挂在无模块下的 endpoint（节点已删，或
 * 测试器的空状态）`moduleId` 为 `null`，这种模块没有格子，只有第 1 步能出值。
 *
 * `source` 报的是**哪一行选中了服务**，不是哪个格子提供了 URL：endpoint 命名
 * 的服务是 `'definition'`，靠省略命中的项目 ★ 是 `'default'`，没解析出是 `'none'`。
 *
 * 对应 `crates/simple-post-mcp/src/variable_fill.rs` 的 `resolve_base_url_with`，
 * 两者必须一致。
 */
export function resolveEndpointBaseUrl(
  endpoint: Pick<EndpointDetail, 'environmentId' | 'baseUrlDefinitionId' | 'baseUrlOverride'>,
  env: Pick<Environment, 'moduleBaseUrls'> | undefined,
  project: Pick<Project, 'baseUrlDefinitions' | 'defaultBaseUrlDefinitionId'> | undefined,
  /**
   * endpoint 所处的模块，由调用方用 `moduleIdOfEndpoint` 解析——这是树的问题，
   * 本模块刻意不懂树。
   */
  moduleId: string | null,
): ResolvedBaseUrl {
  if (endpoint.baseUrlOverride !== null && endpoint.baseUrlOverride !== undefined) {
    return {
      baseUrl: endpoint.baseUrlOverride,
      source: 'override',
      definitionId: null,
      definitionName: null,
    };
  }
  // `null`（未选择）落到项目 ★。全应用只有这一处读项目默认服务，且只读一次。
  const defId = endpoint.baseUrlDefinitionId ?? project?.defaultBaseUrlDefinitionId ?? null;
  const def = defId ? project?.baseUrlDefinitions.find((d) => d.id === defId) : undefined;
  if (!def) return { baseUrl: '', source: 'none', definitionId: null, definitionName: null };
  const url = moduleId ? (env?.moduleBaseUrls?.[def.id]?.[moduleId] ?? '') : '';
  if (!url) return { baseUrl: '', source: 'none', definitionId: null, definitionName: null };
  return {
    baseUrl: url,
    source: endpoint.baseUrlDefinitionId ? 'definition' : 'default',
    definitionId: def.id,
    definitionName: def.name,
  };
}

/**
 * 安全地拼接 baseUrl 与请求 path。path 自带 scheme 时视作绝对地址，并折叠
 * join 边界上的重复斜杠。
 *
 *   joinBaseUrl('https://api.example.com', '/users/1')
 *     → 'https://api.example.com/users/1'
 *   joinBaseUrl('https://api.example.com/', 'users/1')
 *     → 'https://api.example.com/users/1'
 *   joinBaseUrl('', '/users/1')
 *     → '/users/1'
 *   joinBaseUrl('https://api.example.com', 'https://other/')
 *     → 'https://other/'  （path 优先——它自带 scheme）
 */
export function joinBaseUrl(baseUrl: string, path: string): string {
  const trimmedPath = path ?? '';
  if (!trimmedPath) return baseUrl ?? '';
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(trimmedPath)) return trimmedPath;
  const trimmedBase = (baseUrl ?? '').replace(/\/+$/, '');
  if (!trimmedBase) return trimmedPath;
  if (trimmedPath.startsWith('/')) return trimmedBase + trimmedPath;
  return `${trimmedBase}/${trimmedPath}`;
}

/**
 * 折叠 URL **path** 里连续的 `/`，输入必须已 join 且已解析 `{{var}}`，
 * 其余字节一概不动。
 *
 * join 必然发生在替换之前（环境自身的 base URL 可能是模板），所以
 * `joinBaseUrl` 无从知道 `{{var}}` 会展开成以 `/` 开头的值。它照样插入分隔符，
 * 替换随后把 `http://host:9200/{{var}}` 变成 `http://host:9200//api`，对多数
 * 服务端那是另一条路由（往往是重定向或 404）。只有替换之后真相才知道，
 * 所以这是独立一遍，而不是让 `joinBaseUrl` 去猜。
 *
 * 范围刻意收窄——无脑 `replace(/\/{2,}/g, '/')` 会破坏真实 URL：
 *   - `scheme://` 不是 path，`https://` 必须原样保留；
 *   - 无 scheme 的前导 `//` 是 protocol-relative 引用（`//cdn.example.com/x`），
 *     它的斜杠有意义；
 *   - `?query` / `#fragment` 是数据不是 path：携带嵌套 URL 的参数
 *     （`?redirect=https://x//y`）必须原样穿过；
 *   - 未解析的 `{{var}}` 标记是文本不是分隔符，由 `missingVars` 报告，不在这里压平。
 *
 * 对应 `crates/simple-post-mcp/src/variable_fill.rs` 的 `collapse_url_slashes`；
 * 两者不一致，预览与真正发出的请求就会描述不同的 URL。
 */
export function collapseUrlSlashes(url: string): string {
  if (!url) return url ?? '';
  // `scheme://` 与 protocol-relative 的 `//` 是头部，不是 path。
  const scheme = /^[a-z][a-z0-9+.-]*:\/\//i.exec(url);
  let head = '';
  let rest = url;
  if (scheme) {
    head = scheme[0];
    rest = url.slice(scheme[0].length);
  } else if (url.startsWith('//')) {
    head = '//';
    rest = url.slice(2);
  }
  // authority 内不含 `/`（到第一个 `/` 就结束），所以头部之后可安全当 path。
  const cut = rest.search(/[?#]/);
  const path = cut === -1 ? rest : rest.slice(0, cut);
  const tail = cut === -1 ? '' : rest.slice(cut);
  return head + path.replace(/\/{2,}/g, '/') + tail;
}

export interface ResolvedVars {
  value: string;
  /**
   * 未解析的 `{{name}}`，按首次出现去重，顺序在多次渲染间稳定（每次敲键都
   * 重排的告警胶囊比没有更糟）。
   *
   * 它是 `crates/simple-post-mcp/src/variable_fill.rs` 里
   * `ResolvedTemplate.missing` 的渲染器孪生；两者不一致，同一 endpoint 在前端
   * 的「未解析变量」提示与 MCP 的 `missing_vars` 报告就不是一回事。
   */
  missing: string[];
}

/**
 * 把 `{{varName}}` 形式的字符串对着变量池解析，并报告解析不到的标记。变量池可以是
 * `Environment`（用 `env.variables`），也可以是裸的 `Record<string,string>`。
 *
 * 未知变量原样保留（如 `{{missing}}` 仍是 `{{missing}}`），让用户在请求预览里
 * 看得见缺什么，而不是静默变成 `undefined`。
 *
 * 变量名只接受 `\w+`（`[A-Za-z0-9_]+`）；`{{ my-var }}`、`{{ a.b }}` 原样穿过。
 * 替换是**单趟、非递归**的——`{{{{a}}}}` 变成 `{{a}}`，不会再展开一次。这与
 * Rust 解析器的 `template_replace` / `is_word` 一致，也是变量值里恰好含 `{{`
 * 时不会被二次展开（或死循环）的原因。
 */
export function resolveVarsWithMissing(
  input: string,
  vars: Environment | Record<string, string> | undefined,
): ResolvedVars {
  if (!input) return { value: '', missing: [] };
  // `Environment` 的变量在 `.variables` 下，裸 record 本身就是映射。用
  // `Environment.baseUrls`（嵌套对象）区分——裸 `Record<string,string>` 在那里
  // 会得到 `string`
  const map: Record<string, string> | undefined = isEnvironment(vars) ? vars.variables : vars;
  const missing: string[] = [];
  const seen = new Set<string>();
  const value = input.replace(/\{\{(\w+)\}\}/g, (_, key: string) => {
    const v = map?.[key];
    // 判空用 nullish 而非真值：显式定义为空串的变量解析为 '' 且**不算**缺失，
    // 只有键不存在才把标记留在原地
    if (v !== undefined && v !== null) return v;
    if (!seen.has(key)) {
      seen.add(key);
      missing.push(key);
    }
    return `{{${key}}}`;
  });
  return { value, missing };
}

/**
 * `resolveVarsWithMissing`，丢弃 missing 列表。保留这个名字是因为既有调用方都
 * import 它——两者同一实现，`{{x}}` 不可能在这里存活却被那里报出来。
 */
export function resolveVars(
  input: string,
  vars: Environment | Record<string, string> | undefined,
): string {
  return resolveVarsWithMissing(input, vars).value;
}

function isEnvironment(v: Environment | Record<string, string> | undefined): v is Environment {
  return (
    !!v &&
    typeof v === 'object' &&
    typeof (v as Environment).baseUrls === 'object' &&
    !Array.isArray((v as Environment).baseUrls)
  );
}

// 重导出面向选择器的类型，调用方就不必再引第二条路径
export type { BaseUrlDefinition };
