/**
 * 从「点选 JSON 树上的一个节点」到「抽取规则 / 处理脚本片段」的纯计算。
 *
 * 这里产出的路径会被原样写进「抽取规则」，再由后端交给 `serde_json_path` 解析，所以语法
 * 限制由解析器决定，不由我们的审美决定：`serde_json_path` 的点号简写只接受 ASCII 字母、
 * 数字与下划线（`$`、中文、空格都不行），于是这些键一律走括号形式 `['...']`。换言之
 * 「安全标识符」是 `^[A-Za-z_][A-Za-z0-9_]*$` —— 比对名字更宽松的 `\w` 直觉更严一点，
 * 因为宽松一分就会生成一条后端读不懂的路径。
 *
 * 脚本片段是相对 `pm.data`（**抽取后**的值）写的：节点路径若落在当前抽取规则之下，就去掉
 * 那截前缀；否则只能回退到 `pm.response.json()`（原始响应）并提示它不受抽取规则影响。
 */

/** 能直接写成 `.key` 的键名。与 `serde_json_path` 的点号简写一致，见文件头。 */
const BARE_KEY = /^[A-Za-z_][A-Za-z0-9_]*$/;

/** 根路径。抽取规则留空时后端也回落到它（`core::dynamic::extract_value`）。 */
export const ROOT_PATH = '$';

/** 一段可粘进「处理脚本」的代码，以及它依赖什么。 */
export interface ScriptSnippet {
  /** 待写入处理脚本的整段代码（含 `return`）。 */
  code: string;
  /** 片段是否走 `pm.data`（即依赖当前抽取规则）。 */
  usesExtract: boolean;
  /** 给用户的一句说明：这条片段以什么为起点。 */
  hint: string;
}

/** 这个键能不能写成点号形式。 */
export function isBareKey(key: string): boolean {
  return BARE_KEY.test(key);
}

/** 把键名转义进括号形式 `['...']` 的内部文本（不含两侧的 `['`、`']`）。 */
export function escapeKey(key: string): string {
  let out = '';
  for (const char of key) {
    const code = char.codePointAt(0) ?? 0;
    if (char === '\\') out += '\\\\';
    else if (char === "'") out += "\\'";
    else if (char === '\n') out += '\\n';
    else if (char === '\r') out += '\\r';
    else if (char === '\t') out += '\\t';
    else if (char === '\b') out += '\\b';
    else if (char === '\f') out += '\\f';
    // 其余控制字符不能用原字符写进 JSONPath 字符串字面量（RFC 9535 明确禁止）。
    else if (code < 0x20) out += `\\u${code.toString(16).padStart(4, '0')}`;
    else out += char;
  }
  return out;
}

/** 在一个对象的父路径后接上某个键。 */
export function pathForKey(parentPath: string, key: string): string {
  const parent = normalizePath(parentPath);
  return isBareKey(key) ? `${parent}.${key}` : `${parent}['${escapeKey(key)}']`;
}

/** 在一个数组的父路径后接上下标。 */
export function pathForIndex(parentPath: string, index: number): string {
  if (!Number.isInteger(index) || index < 0) {
    throw new RangeError(`数组下标必须是非负整数：${index}`);
  }
  return `${normalizePath(parentPath)}[${index}]`;
}

/** 从根开始一路接下去（`['a', 0, 'b b']` → `$.a[0]['b b']`）。 */
export function pathForSegments(segments: ReadonlyArray<string | number>): string {
  let path = ROOT_PATH;
  for (const segment of segments) {
    path = typeof segment === 'number' ? pathForIndex(path, segment) : pathForKey(path, segment);
  }
  return path;
}

/** 规范化抽取规则：全空白视作「没填」，即根。 */
export function normalizePath(path: string | null | undefined): string {
  const trimmed = (path ?? '').trim();
  return trimmed === '' ? ROOT_PATH : trimmed;
}

/**
 * `nodePath` 是否落在 `basePath` 之下（含相等）。
 *
 * 只按前缀比较是不够的：`$.ab` 不是 `$.a` 的后代。故要求剩下的那截以 `.` 或 `[` 开头。
 */
export function isUnderPath(nodePath: string, basePath: string): boolean {
  const node = normalizePath(nodePath);
  const base = normalizePath(basePath);
  if (node === base) return true;
  if (!node.startsWith(base)) return false;
  const rest = node.slice(base.length);
  return rest.startsWith('.') || rest.startsWith('[');
}

/**
 * 生成处理脚本片段：一份**诚实的起步模板**。
 *
 * 模板原样保存时语义与「不写脚本」相同——末句 `return pm.data;` 返回的就是抽取结果本身，
 * 故点完按钮再保存不会悄悄改掉取值。模板只负责把三样输入点到：抽取结果（`pm.data`）、
 * 原始响应（`pm.response.text()` / `json()`），以及用户刚点的那一格；常用加工留成注释。
 *
 * `nodePath` 来自 JSON 树（必以 `$` 开头）；以 `$` 之外的写法传入时按「根下的相对写法」
 * 处理，免得拼出一条后端读不懂的路径。
 */
export function buildScriptSnippet(nodePath: string, extractPath: string): ScriptSnippet {
  const node = absolutePath(nodePath);
  const extract = normalizePath(extractPath);
  const usesExtract = isUnderPath(node, extract);
  // 刚点的那一格在当前上下文里的表达式：落进抽取规则就相对 `pm.data`，否则得读原始响应。
  const picked = usesExtract
    ? `pm.data${node.slice(extract.length)}`
    : `pm.response.json()${node.slice(ROOT_PATH.length)}`;
  // 默认语句只有一句 `return pm.data;`：模板一旦自带加工，用户点完保存就换掉了取值。
  const code = [
    '// 起点：原样返回抽取结果，与不写脚本等价 —— 据此改出你要的值。',
    '// 抽取结果 pm.data；原始响应 pm.response.text() / pm.response.json()。',
    `// 点选的这一格：${picked}`,
    '// 常用加工：return pm.data.items.map((x) => x.code).join(",");',
    'return pm.data;',
  ].join('\n');
  return {
    code,
    usesExtract,
    hint: usesExtract
      ? '以抽取结果（pm.data）为起点；原样返回等于不写脚本。'
      : '这一格不在抽取结果里，得从原始响应取（pm.response.json()）；这段不依赖抽取规则，模板仍原样返回 pm.data。',
  };
}

/**
 * 覆盖已有脚本前要不要确认。
 *
 * 用户可能手写过脚本，预览面板的「生成片段」只是便利，不能替人做主。
 */
export function shouldConfirmOverwrite(existingScript: string | null | undefined): boolean {
  return (existingScript ?? '').trim() !== '';
}

/** 把任意写法收敛成绝对路径：缺 `$` 的按根下相对写法补上。 */
function absolutePath(path: string): string {
  const trimmed = (path ?? '').trim();
  if (trimmed === '') return ROOT_PATH;
  if (trimmed.startsWith(ROOT_PATH)) return trimmed;
  return trimmed.startsWith('.') || trimmed.startsWith('[')
    ? `${ROOT_PATH}${trimmed}`
    : `${ROOT_PATH}.${trimmed}`;
}
