/**
 * URL 辅助 —— 纯函数、无框架依赖。HttpTester、BaseUrlPicker 与 cURL 导出共用
 * 同一套 join / 解析逻辑；要改解析规则就只在这里改一次。
 */
import type { EndpointDetail, Environment, RewriteRule } from '@/types/domain';

/**
 * 生效的 baseUrl 来自哪一层；选择器用它标注当前选择（「跟随模块」/「自定义」）。`override` 是端点
 * 自己的 `baseUrlOverride`，`module` 是环境里「环境 × 模块」那一格，`none` 是谁都没匹配上。
 */
export type BaseUrlSource = 'override' | 'module' | 'none';

export interface ResolvedBaseUrl {
  /** 模块地址。 */
  baseUrl: string;
  /** 有序的路径前缀改写规则，按顺序匹配、命中首条即停止。空 = 没有改写。 */
  rewrites: RewriteRule[];
  source: BaseUrlSource;
}

/**
 * 解析 endpoint 生效的 baseUrl。解析顺序（优先级由高到低）：`endpoint.baseUrlOverride`（每 endpoint
 * 的「自定义」模式）→ `env.moduleBaseUrls[moduleId]`（环境 × 模块那一格）→ `''`（什么都没有）。
 *
 * override 是三态：`null` / `undefined` = 不在自定义模式，继续读模块那一格；`''` = 自定义但不加前缀
 * （URL 字段本身就是完整地址）；`'https://x'` = 自定义且带显式主机前缀。这正是 `double_option` patch
 * 约定在干活：调用方写 `baseUrlOverride: ''` 是**记录这个选择**而不是清空，只有 `null` 才清空。
 *
 * 值住在（环境 × 模块）这一格，每格独立，**没有回退层**：没有项目默认，不借用别的模块的格子。
 * 挂在无模块下的 endpoint `moduleId` 为 `null`，这种模块没有格子，只有第 1 步能出值。
 *
 * `source` 报的是**哪一层选中了地址**。对应 `crates/simple-post-mcp/src/variable_fill/base_url.rs`
 * 的 `resolve_base_url`，两者必须一致。
 */
export function resolveEndpointBaseUrl(
  endpoint: Pick<EndpointDetail, 'baseUrlOverride'>,
  env: Pick<Environment, 'moduleBaseUrls'> | undefined,
  /**
   * endpoint 所处的模块，由调用方用 `moduleIdOfEndpoint` 解析——这是树的问题，
   * 本模块刻意不懂树。
   */
  moduleId: string | null,
): ResolvedBaseUrl {
  const cell = moduleId ? env?.moduleBaseUrls?.[moduleId] : undefined;
  // 路径前缀改写规则只作用在端点自己的 URL 上，与地址来源无关：端点自带自定义主机时，
  // 路径照样按这一格的规则改写——主机是端点自己的事，路径前缀是这一格的事。
  const rewrites = cell?.rewrites ?? [];
  if (endpoint.baseUrlOverride !== null && endpoint.baseUrlOverride !== undefined) {
    return {
      baseUrl: endpoint.baseUrlOverride,
      rewrites,
      source: 'override',
    };
  }
  if (!moduleId) return { baseUrl: '', rewrites: [], source: 'none' };
  if (!cell || !cell.url) return { baseUrl: '', rewrites, source: 'none' };
  return {
    baseUrl: cell.url,
    rewrites,
    source: 'module',
  };
}

/**
 * 规范化一条改写规则的起点，用于判重。
 *
 * 去首尾空白，再去掉末尾所有 `/`；去掉后为空（如 `"/"`、`"///"`）时返回 `"/"`。
 * 故 `"/api/air"` 与 `"/api/air/"` 是同一个起点，`"/"` 与 `"///"` 也是。与 Rust 侧
 * `core::url::normalize_rewrite_from` 一致。
 */
export function normalizeRewriteFrom(from: string): string {
  const trimmed = from.trim();
  if (trimmed === '') return '';
  const stripped = trimmed.replace(/\/+$/, '');
  return stripped === '' ? '/' : stripped;
}

/** 目标是不是绝对地址。带 `scheme://` 或以 `//` 开头的都算。 */
export function isAbsoluteRewriteTarget(to: string): boolean {
  const trimmed = to.trim();
  return SCHEME_RE.test(trimmed) || trimmed.startsWith('//');
}

/**
 * 单条规则对 URL 的改写结果；不命中返回 `null`。
 *
 * 规则只作用在路径上：主机与查询串一概不动。模块地址自带的路径段不参与匹配。
 *
 * - `from` 去掉首尾空白之后为空 → `null`（没有起点就没有规则）。
 * - `to` 是绝对地址 → `null`。改写只准改路径，不准把请求挪到另一个主机上去——那会把
 *   请求里的凭据发到别处。
 * - `from` 去掉尾部所有 `/`；起点就是 `/` 时它指的是**路径根**，整条路径换到 `to` 底下。
 * - 路径以 `from` 开头，**且**其后紧跟的那一位是 `/`（或路径恰好等于 `from`）→ 换成
 *   `to` + 其余部分。边界这一条是必需的：没有它，`/api/air` 会命中 `/api/airplane`。
 *
 * `from` 按**原文**匹配：它不做 `{{var}}` 替换，故那里的标记永远不会被展开。`to` 会被随后的
 * `{{var}}` 替换扫到（它进了 URL）。
 *
 * 对应 Rust 侧 `simple_post_core::url::try_apply_rewrite`；两者必须一致，否则预览与真正
 * 发出去的请求描述的是两个 URL。共享样本是 `fixtures/base-url-parity.json`。
 */
export function tryApplyRewrite(url: string, rule: RewriteRule): string | null {
  const fromRaw = rule.from.trim();
  if (fromRaw === '') return null;
  const to = rule.to.trim();
  if (isAbsoluteRewriteTarget(to)) return null;

  const { head, path, tail } = splitUrl(url);
  const from = fromRaw.replace(/\/+$/, '');
  if (from === '') {
    // 起点是路径根：整条路径换到目标底下。两边各自带的那道斜杠都要留着，
    // 故这里是直接相接（`/gateway` + `/api/air/list`）。
    return head + to + path + tail;
  }
  if (!path.startsWith(from)) return null;
  const rest = path.slice(from.length);
  if (rest !== '' && !rest.startsWith('/')) return null;
  return head + to + rest + tail;
}

/**
 * 按列表顺序依次尝试改写，**命中第一条后立即返回**（不串联执行后续规则）；
 * 全部未命中时原样返回。与 Rust 侧 `core::url::apply_rewrites` 一致。
 */
export function applyRewrites(
  url: string,
  rules: readonly RewriteRule[] | null | undefined,
): string {
  if (!rules) return url;
  for (const rule of rules) {
    const next = tryApplyRewrite(url, rule);
    if (next !== null) return next;
  }
  return url;
}

export interface RewriteValidationResult {
  /** 行下标 → 就地展示的错误文案。 */
  errorsByIndex: Record<number, string>;
  hasError: boolean;
}

/**
 * 校验一组改写规则（本地草稿形态）。
 *
 * - 全空白草稿行（`from` 与 `to` 都空）被跳过：它是用户刚点「添加」还没填的那一行，
 *   留着等输入，不算错。
 * - 有 `to` 却没 `from` → 该行报错。凭空多出一段前缀是拼接不出请求的。
 * - 目标是绝对地址 → 该行报错（改写只准改路径）。
 * - 同一组内规范化起点重复 → **所有冲突行**都报错：按顺序匹配时后面那条永远不会生效。
 */
export function validateRewriteRules(rules: readonly RewriteRule[]): RewriteValidationResult {
  const errorsByIndex: Record<number, string> = {};
  const byKey = new Map<string, number[]>();
  rules.forEach((rule, index) => {
    if (rule.from.trim() === '' && rule.to.trim() === '') return;
    if (rule.from.trim() === '') {
      errorsByIndex[index] = '请填写要匹配的路径前缀（起点）';
      return;
    }
    if (isAbsoluteRewriteTarget(rule.to)) {
      errorsByIndex[index] = '目标不能包含协议或主机地址（仅改写路径）';
      return;
    }
    const key = normalizeRewriteFrom(rule.from);
    const seen = byKey.get(key);
    if (seen) seen.push(index);
    else byKey.set(key, [index]);
  });
  for (const [key, indices] of byKey) {
    if (indices.length < 2) continue;
    for (const index of indices) {
      errorsByIndex[index] = `起点「${key}」重复，按顺序匹配时后续规则不会生效`;
    }
  }
  return { errorsByIndex, hasError: Object.keys(errorsByIndex).length > 0 };
}

/**
 * 找出一组规则里被前置规则遮挡的行下标（非阻断提示，不阻止保存）。
 *
 * 若更靠前的第 `j` 条规则的规范化起点是第 `i` 条的路径祖先前缀（`"/"` 排在
 * `"/api/air"` 之前，或 `"/api"` 排在 `"/api/air"` 之前），返回 `i -> j`：按顺序匹配时
 * 第 `i` 条可能永远走不到，界面提示用户拖拽调整顺序。
 */
export function findShadowedRuleIndices(rules: readonly RewriteRule[]): Record<number, number> {
  const keys = rules.map((rule) => normalizeRewriteFrom(rule.from));
  const shadowed: Record<number, number> = {};
  for (let i = 0; i < keys.length; i += 1) {
    const key = keys[i];
    if (key === '' || key in shadowed) continue;
    for (let j = 0; j < i; j += 1) {
      const outer = keys[j];
      if (outer === '') continue;
      const covers = outer === '/' || key === outer || key.startsWith(`${outer}/`);
      if (covers) {
        shadowed[i] = j;
        break;
      }
    }
  }
  return shadowed;
}

/** 前导 `scheme://`。与 Rust 侧 `core::url::scheme_prefix_len` 认同一件事。 */
const SCHEME_RE = /^[a-z][a-z0-9+.-]*:\/\//i;

/**
 * 把 URL 拆成三段：主机部分（含 scheme）、路径、查询串与片段。
 *
 * 主机部分含 scheme 与 authority：`https://h`、`//h`、空串（相对地址）都算主机部分。
 * 路径是从主机部分之后到 `?` 或 `#` 之前的那一段。
 */
function splitUrl(url: string): { head: string; path: string; tail: string } {
  let head = '';
  let rest = url;
  const scheme = SCHEME_RE.exec(url);
  if (scheme) {
    const after = url.slice(scheme[0].length);
    const end = after.search(/[/?#]/);
    const cut = end === -1 ? after.length : end;
    head = url.slice(0, scheme[0].length + cut);
    rest = url.slice(head.length);
  } else if (url.startsWith('//')) {
    const after = url.slice(2);
    const end = after.search(/[/?#]/);
    const cut = end === -1 ? after.length : end;
    head = url.slice(0, 2 + cut);
    rest = url.slice(head.length);
  }
  const cut = rest.search(/[?#]/);
  return {
    head,
    path: cut === -1 ? rest : rest.slice(0, cut),
    tail: cut === -1 ? '' : rest.slice(cut),
  };
}

/**
 * 安全地拼接 baseUrl 与请求 path。path 自带 scheme 时视作绝对地址，并折叠 join 边界上的重复斜杠。
 *
 *   示例：
 *   - joinBaseUrl('https://api.example.com', '/users/1') → 'https://api.example.com/users/1'
 *   - joinBaseUrl('https://api.example.com/', 'users/1') → 'https://api.example.com/users/1'
 *   - joinBaseUrl('', '/users/1') → '/users/1'
 *   - joinBaseUrl('https://api.example.com', 'https://other/') → 'https://other/'（path 优先）
 */
export function joinBaseUrl(baseUrl: string, path: string): string {
  const trimmedPath = path ?? '';
  if (!trimmedPath) return baseUrl ?? '';
  if (SCHEME_RE.test(trimmedPath)) return trimmedPath;
  const trimmedBase = (baseUrl ?? '').replace(/\/+$/, '');
  if (!trimmedBase) return trimmedPath;
  if (trimmedPath.startsWith('/')) return trimmedBase + trimmedPath;
  return `${trimmedBase}/${trimmedPath}`;
}

/**
 * 折叠 URL **path** 里连续的 `/`，输入必须已 join 且已解析 `{{var}}`，其余字节一概不动。
 *
 * join 必然发生在替换之前（环境自身的 base URL 可能是模板），故 `joinBaseUrl` 无从知道
 * `{{var}}` 会展开成以 `/` 开头的值；只有替换之后真相才知道，所以这是独立一遍，而不是让
 * `joinBaseUrl` 去猜。
 *
 * 范围刻意收窄——无脑 `replace(/\/{2,}/g, '/')` 会破坏真实 URL：
 *
 * - `scheme://` 不是 path，`https://` 必须原样保留。
 * - 无 scheme 的前导 `//` 是 protocol-relative 引用，它的斜杠有意义。
 * - `?query` / `#fragment` 是数据不是 path：携带嵌套 URL 的参数必须原样穿过。
 * - 未解析的 `{{var}}` 标记是文本不是分隔符，由 `missingVars` 报告，不在这里压平。
 *
 * 对应 `crates/simple-post-mcp/src/variable_fill.rs` 的 `collapse_url_slashes`；两者不一致，
 * 预览与真正发出的请求就会描述不同的 URL。
 */
export function collapseUrlSlashes(url: string): string {
  if (!url) return url ?? '';
  // `scheme://` 与 protocol-relative 的 `//` 归入 `head`，不是 path。
  const scheme = SCHEME_RE.exec(url);
  let head = '';
  let rest = url;
  if (scheme) {
    head = scheme[0];
    rest = url.slice(scheme[0].length);
  } else if (url.startsWith('//')) {
    head = '//';
    rest = url.slice(2);
  }
  // authority 内不含 `/`（到第一个 `/` 就结束），所以 `head` 之后可安全当 path。
  const cut = rest.search(/[?#]/);
  const path = cut === -1 ? rest : rest.slice(0, cut);
  const tail = cut === -1 ? '' : rest.slice(cut);
  return head + path.replace(/\/{2,}/g, '/') + tail;
}

export interface ResolvedVars {
  value: string;
  /**
   * 未解析的 `{{name}}`，按首次出现去重，顺序在多次渲染间稳定（每次敲键都重排的告警胶囊比没有更糟）。
   * 它是 `crates/simple-post-mcp/src/variable_fill.rs` 里 `ResolvedTemplate.missing` 的渲染器孪生，
   * 两者不一致时前端提示与 MCP 的 `missing_vars` 报告就不是一回事。
   */
  missing: string[];
}

/**
 * 把 `{{varName}}` 形式的字符串对着变量池解析，并报告解析不到的标记。变量池可以是
 * `Environment`（用 `env.variables`），也可以是裸的 `Record<string,string>`。
 *
 * - 未知变量原样保留（如 `{{missing}}` 仍是 `{{missing}}`），让用户在请求预览里看得见缺什么。
 * - **停用的变量不参与替换**：它们照样存、照样显示，但 `{{x}}` 留在原样并进 `missing` 报告。
 * - 变量名只接受 `\w+`（`[A-Za-z0-9_]+`）；`{{ my-var }}`、`{{ a.b }}` 原样穿过。
 * - 替换是**单趟、非递归**的——`{{{{a}}}}` 变成 `{{a}}`，不会再展开一次。这与 Rust 解析器的
 *   `template_replace` / `is_word` 一致，也是变量值里恰好含 `{{` 时不会二次展开的原因。
 */
export function resolveVarsWithMissing(
  input: string,
  vars: Environment | Record<string, string> | undefined,
): ResolvedVars {
  if (!input) return { value: '', missing: [] };
  // `Environment` 的变量在 `.variables` 下，裸 record 本身就是映射；`isEnvironment` 用
  // `Environment.moduleBaseUrls`（对象）区分两者——裸 `Record<string,string>` 在那里会得到 `undefined`
  const map: Record<string, string> | undefined = isEnvironment(vars) ? enabledVars(vars) : vars;
  const missing: string[] = [];
  const seen = new Set<string>();
  const value = input.replace(/\{\{(\w+)\}\}/g, (_, key: string) => {
    const v = map?.[key];
    // 判空用 nullish 而非真值：显式定义为空串的变量解析为 '' 且**不算**缺失，只有键不存在时替换回调才把标记原样留在原处
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
 * 列出字符串里出现的**全部** `{{name}}` 标记名，按首次出现去重。
 *
 * 与 [`resolveVarsWithMissing`] 的区别是：那个只报告**没解析出来**的名字，这个报告**出现过**的名字。
 * 动态变量要知道自己有没有被请求引用，就得问这个——一个同名的环境变量会让 `{{name}}` 解析成功，
 * 于是「没解析出来」那个清单里根本没有它。
 *
 * 判据与 Rust 的 `core::template::list_missing_vars` 同一套：找**配对**的 `}}`，中间的内容必须是裸
 * 标识符（`[A-Za-z0-9_]+`）。故叠写的形状（`{{{{a}}}}`）不算标记——它先遇到的那对 `}}` 截出的是
 * `{{a`，不是标识符。两侧必须逐条相同，故这份判据由 `fixtures/dynamic-parity.json` 的 `scanCases` 钉住。
 */
export function listVars(input: string): string[] {
  if (!input) return [];
  const out: string[] = [];
  const seen = new Set<string>();
  let i = 0;
  while (i < input.length) {
    if (input[i] === '{' && input[i + 1] === '{') {
      const close = input.indexOf('}}', i + 2);
      if (close !== -1) {
        const inner = input.slice(i + 2, close);
        if (WORD.test(inner) && !seen.has(inner)) {
          seen.add(inner);
          out.push(inner);
        }
        // `listVars` 跳过整个标记（含那对配对的大括号），与 Rust 侧一样。
        i = close + 2;
        continue;
      }
    }
    i += 1;
  }
  return out;
}

/** 裸标识符：字母、数字与下划线，且非空。与 Rust 的 `template::is_word` 同一套判据。 */
const WORD = /^[A-Za-z0-9_]+$/;

/**
 * `resolveVarsWithMissing`，丢弃 missing 列表。两者是同一段代码：`{{x}}` 不可能在这一处存活
 * 却在那一处被报出来。共享样本的读取器 import 两者并比对结果（见 `__tests__/url.test.ts`）。
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
    typeof (v as Environment).moduleBaseUrls === 'object' &&
    !Array.isArray((v as Environment).moduleBaseUrls)
  );
}

/**
 * 参与替换的变量：环境里**没有**被停用的那些。停用的变量照样存、照样显示，但不参与替换——
 * `{{x}}` 留在原样并进 `missing` 报告，用户看得见「这个变量没被替换」。
 *
 * 导出是因为脚本看到的变量池必须是**同一份**：`pm.environment.get('token')` 读到的必须是这次
 * 请求替换进去的那个值。
 */
export function enabledVars(env: Environment): Record<string, string> {
  const disabled = env.disabledVariables ?? [];
  if (disabled.length === 0) return env.variables;
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(env.variables)) {
    if (!disabled.includes(key)) out[key] = value;
  }
  return out;
}
