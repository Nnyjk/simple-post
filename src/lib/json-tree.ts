/**
 * JSON 响应体的键值树模型。
 *
 * 模型只做两件纯事：把解析好的值摊成一棵带 JSON 路径的节点树，再按展开状态摊平成行。
 * 两者纯函数化是为了被测试钉住 —— 前端测试没有 DOM，路径、预览文本与子树 JSON 只能
 * 在模型这一层断言。
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
  value: JsonValue;
  /** 标量节点显示成一行的那段文本；容器为 null。 */
  scalarText: string | null;
  /** 折叠时跟在容器键名后面的摘要；标量为 null。 */
  preview: string | null;
  children: JsonTreeNode[];
  /** 该节点整棵子树格式化（缩进 2）后的 JSON。 */
  json: string;
}

export interface FlatJsonRow {
  node: JsonTreeNode;
  /** 根节点为 0，每下一层加 1。 */
  depth: number;
}

export type JsonTreeResult = { ok: true; root: JsonTreeNode } | { ok: false; error: string };

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
  if (count === 0) return isArray ? '[]' : '{}';
  return isArray ? `[…] ${count} 项` : `{…} ${count} 项`;
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

function makeNode(
  value: JsonValue,
  key: string | null,
  index: number | null,
  path: string,
): JsonTreeNode {
  const kind = kindOf(value);
  const children: JsonTreeNode[] = [];
  if (kind === 'array') {
    (value as JsonValue[]).forEach((item, i) => {
      children.push(makeNode(item, null, i, childPath(path, i)));
    });
  } else if (kind === 'object') {
    for (const [k, v] of Object.entries(value as Record<string, JsonValue>)) {
      children.push(makeNode(v, k, null, childPath(path, k)));
    }
  }
  const isScalar = kind === 'scalar';
  return {
    kind,
    key,
    index,
    path,
    value,
    scalarText: isScalar ? scalarText(value) : null,
    preview: isScalar ? null : containerPreview(value),
    children,
    json: JSON.stringify(value, null, 2) ?? String(value),
  };
}

/** 把任意解析结果摊成以 `$` 为根的树。 */
export function buildJsonTree(value: unknown): JsonTreeNode {
  return makeNode(value as JsonValue, null, null, '$');
}

/**
 * 把响应文本解析成树。空文本与非法 JSON 都以带原因的结果返回，调用方不必 try/catch。
 */
export function parseJsonTree(text: string): JsonTreeResult {
  const s = text.trim();
  if (!s) return { ok: false, error: '内容为空' };
  try {
    return { ok: true, root: buildJsonTree(JSON.parse(s)) };
  } catch (e) {
    return { ok: false, error: (e as Error).message };
  }
}

/**
 * 节点值的纯文本：字符串取原文（不带引号、不再转义），其它标量取 JSON 字面量，容器取
 * 子树格式化后的 JSON。复制「值」与「键值对」都用它。
 */
export function valueText(node: JsonTreeNode): string {
  if (node.kind !== 'scalar') return node.json;
  return typeof node.value === 'string' ? node.value : String(node.value);
}

/** 键值对的文本：有键名时写作 `key: value`，数组元素用作下标，根节点只给值。 */
export function pairText(node: JsonTreeNode): string {
  const label = node.key ?? (node.index === null ? null : String(node.index));
  return label === null ? valueText(node) : `${label}: ${valueText(node)}`;
}

/**
 * 按展开判断把树摊平成行，折叠节点的子孙不出现在结果里。行序即树的中序遍历（先父后子）。
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
