/**
 * JSON 响应体的键值树模型。
 *
 * 模型只做三件纯事：把文本解析成一棵带 JSON 路径的节点树、按展开状态摊平成行、把任意
 * 子树写回格式化 JSON。三者纯函数化是为了被测试钉住 —— 前端测试没有 DOM，路径、预览文本
 * 与子树 JSON 只能在模型这一层断言。
 *
 * 解析器自己写，不用 `JSON.parse`：后者把 `9007199254740993` 抹成 `9007199254740992`，
 * `1.0` 与 `1e10` 的形状也一并丢掉。这两种写法恰恰是服务端原样写出来的，用户复制「值」
 * 或「子树 JSON」时该拿到服务端写的那一份，而不是浏览器四舍五入后的那一份。故每个标量
 * 节点都留着源码里的那个 token（`scalarText`）。
 */

export type JsonValue =
  null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export type JsonNodeKind = 'object' | 'array' | 'scalar';

export interface JsonTreeNode {
  kind: JsonNodeKind;
  /** 对象成员的键名；数组元素、根节点为 null。 */
  key: string | null;
  /** 数组元素在父数组里的下标；其余节点为 null。 */
  index: number | null;
  /** JSON 路径，根为 `$`，数组元素写作 `[i]`。 */
  path: string;
  /**
   * 标量的值（字符串是解码后的那一份，数字是按 token 解析出的 double）。容器为 `null`：
   * 容器就是它的 `children`，再存一份同构的值等于把整份响应复制两遍。
   */
  value: JsonValue;
  /**
   * 标量在源码里的那个 token；容器为 null。
   *
   * 字符串存 `JSON.stringify` 的转义形式（带引号），数字存原文，故 `1.0` / `1e10` /
   * `9007199254740993` 都按服务端写的形状显示与复制。
   */
  scalarText: string | null;
  /** 折叠时跟在容器键名后面的摘要；标量为 null。 */
  preview: string | null;
  children: JsonTreeNode[];
}

export interface FlatJsonRow {
  node: JsonTreeNode;
  /** 根节点为 0，每下一层加 1。 */
  depth: number;
}

export type JsonTreeResult = { ok: true; root: JsonTreeNode } | { ok: false; error: string };

/**
 * 解析结果里还没摊成节点的形状：标量连着它的显示文本一起走，容器只带子项。
 *
 * 单独一层是因为「摊成节点」要算路径与预览，而路径只有在知道父路径时才成立 —— 解析器
 * 只认文本，不认路径。
 */
type ParsedValue =
  | { kind: 'scalar'; value: JsonValue; text: string }
  | { kind: 'array'; items: ParsedValue[] }
  | { kind: 'object'; entries: [string, ParsedValue][] };

/** 能直接写成 `obj.key` 的键名；其余键名要用 `obj["…"]` 的写法。 */
const IDENTIFIER = /^[A-Za-z_$][A-Za-z0-9_$]*$/;

/** 标量显示成一行的那段文本：字符串带引号与转义，其余取 JSON 字面量。 */
export function scalarText(value: JsonValue): string {
  return typeof value === 'string' ? JSON.stringify(value) : String(value);
}

/** 容器折叠时的摘要，如 `{…} 3 项`；空容器直接写成 `{}` / `[]`。 */
export function containerPreview(value: JsonValue): string {
  const isArray = Array.isArray(value);
  const count = isArray ? value.length : Object.keys(value as Record<string, JsonValue>).length;
  return previewOf(isArray ? 'array' : 'object', count);
}

function previewOf(kind: 'array' | 'object', count: number): string {
  if (count === 0) return kind === 'array' ? '[]' : '{}';
  return kind === 'array' ? `[…] ${count} 项` : `{…} ${count} 项`;
}

/** 子节点的路径：数组下标写作 `[i]`，标识符键名写作 `.key`，其余键名加引号塞进 `[…]`。 */
export function childPath(parentPath: string, key: string | number): string {
  if (typeof key === 'number') return `${parentPath}[${key}]`;
  return IDENTIFIER.test(key) ? `${parentPath}.${key}` : `${parentPath}[${JSON.stringify(key)}]`;
}

function kindOf(value: JsonValue): JsonNodeKind {
  if (Array.isArray(value)) return 'array';
  return value !== null && typeof value === 'object' ? 'object' : 'scalar';
}

function nodeFrom(
  parsed: ParsedValue,
  key: string | null,
  index: number | null,
  path: string,
): JsonTreeNode {
  if (parsed.kind === 'scalar') {
    return {
      kind: 'scalar',
      key,
      index,
      path,
      value: parsed.value,
      scalarText: parsed.text,
      preview: null,
      children: [],
    };
  }
  const children =
    parsed.kind === 'array'
      ? parsed.items.map((item, i) => nodeFrom(item, null, i, childPath(path, i)))
      : parsed.entries.map(([k, v]) => nodeFrom(v, k, null, childPath(path, k)));
  return {
    kind: parsed.kind,
    key,
    index,
    path,
    value: null,
    scalarText: null,
    preview: previewOf(parsed.kind, children.length),
    children,
  };
}

/** 已解析好的 JS 值转成解析器的中间形态：标量没有原文，显示文本由 `scalarText` 现算。 */
function valueToParsed(value: JsonValue): ParsedValue {
  const kind = kindOf(value);
  if (kind === 'array') {
    return { kind: 'array', items: (value as JsonValue[]).map(valueToParsed) };
  }
  if (kind === 'object') {
    return {
      kind: 'object',
      entries: Object.entries(value as Record<string, JsonValue>).map(
        ([k, v]): [string, ParsedValue] => [k, valueToParsed(v)],
      ),
    };
  }
  return { kind: 'scalar', value, text: scalarText(value) };
}

/** 把已解析的值摊成以 `$` 为根的树。已经拿到值的调用方（测试、示例）走这一条。 */
export function buildJsonTree(value: JsonValue): JsonTreeNode {
  return nodeFrom(valueToParsed(value), null, null, '$');
}

/** 解析位置：`s[i]` 是下一个待读的字符。 */
interface Cursor {
  s: string;
  i: number;
}

/** 解析错误只在本文件内抛，出去时被 `parseJsonTree` 收成 `{ ok: false }`。 */
class JsonParseError extends Error {}

function fail(c: Cursor, reason: string): never {
  throw new JsonParseError(`${reason}（第 ${c.i + 1} 个字符）`);
}

const WHITESPACE = ' \t\n\r';

/** 粘性匹配从 `lastIndex` 起读，故先把游标交给它再读。 */
const NUMBER = /-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/y;

const ESCAPES: Record<string, string> = {
  '"': '"',
  '\\': '\\',
  '/': '/',
  b: '\b',
  f: '\f',
  n: '\n',
  r: '\r',
  t: '\t',
};

function skipWhitespace(c: Cursor): void {
  while (c.i < c.s.length && WHITESPACE.includes(c.s[c.i])) c.i += 1;
}

function parseValue(c: Cursor): ParsedValue {
  skipWhitespace(c);
  const ch = c.s[c.i];
  if (ch === undefined) fail(c, '内容意外结束');
  if (ch === '{') return parseObject(c);
  if (ch === '[') return parseArray(c);
  if (ch === '"') {
    const value = parseString(c);
    return { kind: 'scalar', value, text: JSON.stringify(value) };
  }
  if (ch === '-' || (ch >= '0' && ch <= '9')) return parseNumber(c);
  for (const [word, value] of [
    ['true', true],
    ['false', false],
    ['null', null],
  ] as const) {
    if (c.s.startsWith(word, c.i)) {
      c.i += word.length;
      return { kind: 'scalar', value, text: word };
    }
  }
  return fail(c, `无法识别的值 ${JSON.stringify(ch)}`);
}

function parseNumber(c: Cursor): ParsedValue {
  NUMBER.lastIndex = c.i;
  const matched = NUMBER.exec(c.s);
  // 字形不合 JSON 的数字文法（`01`、`.5`、`1.`）在这里就断掉，不留到后面当合法文本。
  if (!matched || matched[0] === '' || matched[0] === '-') fail(c, '数字格式不合法');
  const text = matched[0];
  c.i += text.length;
  return { kind: 'scalar', value: Number(text), text };
}

function parseString(c: Cursor): string {
  const s = c.s;
  c.i += 1; // 跳过开引号
  let out = '';
  for (;;) {
    const ch = s[c.i];
    if (ch === undefined) fail(c, '字符串没有闭合');
    if (ch === '"') {
      c.i += 1;
      return out;
    }
    // JSON 不允许字符串里出现未转义的控制字符（换行、制表符、NUL 都在此列）。
    if (ch < ' ') fail(c, '字符串里出现了未转义的控制字符');
    if (ch !== '\\') {
      out += ch;
      c.i += 1;
      continue;
    }
    c.i += 1;
    const esc = s[c.i];
    if (esc === undefined) fail(c, '字符串没有闭合');
    if (esc === 'u') {
      const hex = s.slice(c.i + 1, c.i + 5);
      if (!/^[0-9a-fA-F]{4}$/.test(hex)) fail(c, '\\u 转义需要四位十六进制');
      // 代理对分两次读进来的正是 UTF-16 的两半，拼起来就是原字符。
      out += String.fromCharCode(parseInt(hex, 16));
      c.i += 5;
      continue;
    }
    const decoded = ESCAPES[esc];
    if (decoded === undefined) fail(c, `无法识别的转义 \\${esc}`);
    out += decoded;
    c.i += 1;
  }
}

function parseArray(c: Cursor): ParsedValue {
  c.i += 1; // 跳过 `[`
  const items: ParsedValue[] = [];
  skipWhitespace(c);
  if (c.s[c.i] === ']') {
    c.i += 1;
    return { kind: 'array', items };
  }
  for (;;) {
    items.push(parseValue(c));
    skipWhitespace(c);
    const ch = c.s[c.i];
    if (ch === ',') {
      c.i += 1;
      continue;
    }
    if (ch === ']') {
      c.i += 1;
      return { kind: 'array', items };
    }
    fail(c, ch === undefined ? '数组没有闭合' : '数组元素之间缺少逗号');
  }
}

function parseObject(c: Cursor): ParsedValue {
  c.i += 1; // 跳过 `{`
  const entries: [string, ParsedValue][] = [];
  skipWhitespace(c);
  if (c.s[c.i] === '}') {
    c.i += 1;
    return { kind: 'object', entries };
  }
  for (;;) {
    skipWhitespace(c);
    if (c.s[c.i] !== '"') fail(c, '对象的键名必须是字符串');
    const key = parseString(c);
    skipWhitespace(c);
    if (c.s[c.i] !== ':') fail(c, '键名后面缺少冒号');
    c.i += 1;
    const value = parseValue(c);
    // 重复键名以最后一次为准，位置留在第一次出现处 —— 与 `JSON.parse` 的结果一致。
    const seen = entries.findIndex(([k]) => k === key);
    if (seen === -1) entries.push([key, value]);
    else entries[seen] = [key, value];
    skipWhitespace(c);
    const ch = c.s[c.i];
    if (ch === ',') {
      c.i += 1;
      continue;
    }
    if (ch === '}') {
      c.i += 1;
      return { kind: 'object', entries };
    }
    fail(c, ch === undefined ? '对象没有闭合' : '对象成员之间缺少逗号');
  }
}

/**
 * 把 JSON 文本解析成树。空文本与非法文本都以带原因的结果返回，调用方不必 try/catch。
 */
export function parseJsonTree(text: string): JsonTreeResult {
  const s = text.trim();
  if (!s) return { ok: false, error: '内容为空' };
  const c: Cursor = { s, i: 0 };
  try {
    const parsed = parseValue(c);
    skipWhitespace(c);
    // 顶层之后再出现任何字符（`{} {}`、`1 2`）都不是一份 JSON。
    if (c.i < s.length) fail(c, '内容结尾还有多余的字符');
    return { ok: true, root: nodeFrom(parsed, null, null, '$') };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}

/** 把一棵子树写成缩进 2 的 JSON。键序按 `children` 的顺序，数字沿用原始 token。 */
export function subtreeJson(node: JsonTreeNode): string {
  return writeJson(node, 0);
}

function writeJson(node: JsonTreeNode, depth: number): string {
  if (node.kind === 'scalar') return node.scalarText ?? scalarText(node.value);
  if (node.children.length === 0) return node.kind === 'array' ? '[]' : '{}';
  const pad = '  '.repeat(depth + 1);
  const close = '  '.repeat(depth);
  const parts = node.children.map((child) => {
    const body = writeJson(child, depth + 1);
    // 数组元素没有键名，故前缀只有对象成员才拼。
    return child.key === null ? pad + body : `${pad}${JSON.stringify(child.key)}: ${body}`;
  });
  return node.kind === 'array'
    ? `[\n${parts.join(',\n')}\n${close}]`
    : `{\n${parts.join(',\n')}\n${close}}`;
}

/**
 * 节点值的纯文本：字符串取原文（不带引号、不再转义），其它标量取源码里的 token，容器取
 * 子树格式化后的 JSON。复制「值」与「键值对」都用它。
 */
export function valueText(node: JsonTreeNode): string {
  if (node.kind !== 'scalar') return subtreeJson(node);
  return typeof node.value === 'string' ? node.value : (node.scalarText ?? String(node.value));
}

/** 键值对的文本：有键名时写作 `key: value`，数组元素用作下标，根节点只给值。 */
export function pairText(node: JsonTreeNode): string {
  const label = node.key ?? (node.index === null ? null : String(node.index));
  return label === null ? valueText(node) : `${label}: ${valueText(node)}`;
}

/**
 * 按展开判断把树摊平成行，折叠节点的子孙不出现在结果里。行序即树的前序遍历（先父后子）。
 */
export function flattenTree(
  root: JsonTreeNode,
  isExpanded: (node: JsonTreeNode) => boolean,
): FlatJsonRow[] {
  const rows: FlatJsonRow[] = [];
  const walk = (node: JsonTreeNode, depth: number) => {
    rows.push({ node, depth });
    if (node.children.length === 0 || !isExpanded(node)) return;
    for (const child of node.children) walk(child, depth + 1);
  };
  walk(root, 0);
  return rows;
}

/** 首屏默认展开的路径：容器节点在深度不超过 `maxDepth` 时展开，根节点深度为 0。 */
export function defaultExpandedPaths(root: JsonTreeNode, maxDepth = 1): string[] {
  const paths: string[] = [];
  const walk = (node: JsonTreeNode, depth: number) => {
    if (node.children.length === 0 || depth > maxDepth) return;
    paths.push(node.path);
    for (const child of node.children) walk(child, depth + 1);
  };
  walk(root, 0);
  return paths;
}
