/**
 * JSON Schema 的结构化视图 —— 把 `RequestBody.schema` 那段文本解析成可以画成表格的形状。
 *
 * 为什么单独一个模块
 * ------------------
 * Schema 文本有三种形态，而界面对三种形态要给出三种不同的结论：
 *
 *   1. 合法且是一份对象定义 —— 画成属性表；
 *   2. 合法但顶层不是对象（顶层是数组或字符串）—— 说清楚「这份定义不是对象」，
 *      而不是画一张没有字段的表；
 *   3. 解析不了 —— 说清楚失败原因，并把用户送回 JSON 视图。
 *
 * 这个模块只做解析与整形，不含 React，也不含 CodeMirror：界面怎么显示是另一件事，
 * 而解析规则本身可以被单独钉住。
 *
 * `$ref` 只显示引用目标的名字
 * --------------------------
 * 界面手上只有这一段文本，整份 OpenAPI 文档不在这里，故 `$ref` 指向的那个定义
 * 本模块看不到。把 `$ref` 展开成假的内容只会骗用户，故这里显示引用目标的名字
 * （例如 `User（引用）`），让用户知道这个字段指向别处定义的哪个类型。
 */

/** 一个字段的展示形状。 */
export interface SchemaField {
  /** 字段名。 */
  name: string;
  /** 类型的展示文本，例如 `string`、`array<string>`、`User（引用）`。 */
  type: string;
  /** 这个字段的名字是否出现在父级的 `required` 列表里。 */
  required: boolean;
  /** `description` 的文字。 */
  description?: string;
  /** `enum` 的取值，逐个转成文本。 */
  enumValues?: string[];
  /** `default` 的取值，转成文本。 */
  defaultValue?: string;
  /** `format` 的取值，例如 `date-time`。 */
  format?: string;
  /** 组合关键字（`oneOf` / `anyOf` / `allOf`）的一句话说明。 */
  note?: string;
  /** 嵌套字段：对象的子属性，或数组元素的属性。 */
  children?: SchemaField[];
}

/** 解析成功时给出的结构。 */
export interface SchemaView {
  /** 根节点的类型文本。 */
  type: string;
  /** 根节点的 `description`。 */
  description?: string;
  /** 根节点列出的字段。 */
  fields: SchemaField[];
  /** 这份定义是不是空的（文本为空串）。 */
  empty: boolean;
  /** 根节点的一句补充说明，例如「下列字段属于数组元素的定义」。 */
  note?: string;
}

/**
 * 解析结果。
 *
 * 解析失败不抛异常：`schema` 是用户手打的文本，坏了是常事而不是程序错误，
 * 故失败也返回一个结果，让界面能显示失败原因。
 */
export type SchemaViewResult =
  | { ok: true; view: SchemaView }
  | { ok: false; kind: 'parse-error' | 'not-an-object'; message: string };

/** 一份 JSON Schema 节点。 */
type SchemaObject = Record<string, unknown>;

function isObject(value: unknown): value is SchemaObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** JSON 值的类型名，写给人看。 */
function jsonTypeName(value: unknown): string {
  if (value === null) return 'null';
  if (Array.isArray(value)) return '数组';
  switch (typeof value) {
    case 'string':
      return '字符串';
    case 'number':
      return '数字';
    case 'boolean':
      return '布尔值';
    case 'object':
      return '对象';
    default:
      return '未知类型';
  }
}

/**
 * 节点的类型文本。
 *
 * `type` 缺失时的两条推断来自 JSON Schema 的常见写法：带 `properties` 的是对象，
 * 带 `items` 的是数组。这两条推断让「没写 type 的 schema」也能读懂。
 */
function typeText(schema: SchemaObject): string {
  const declared = schema.type;
  if (typeof declared === 'string' && declared.trim()) return declared.trim();
  if (Array.isArray(declared)) {
    const parts = declared
      .filter((item): item is string => typeof item === 'string' && item.trim().length > 0)
      .map((item) => item.trim());
    if (parts.length) return parts.join(' | ');
  }
  if (isObject(schema.properties)) return 'object';
  if (schema.items !== undefined) return 'array';
  return 'any';
}

/** `$ref` 指向的名字：取路径的最后一段。 */
function refName(ref: string): string {
  const segments = ref.split('/').filter(Boolean);
  return segments[segments.length - 1] ?? ref;
}

/** 类型在界面上的文本，包含 `$ref` 与数组元素类型。 */
function displayType(schema: SchemaObject): string {
  const ref = schema.$ref;
  if (typeof ref === 'string' && ref.trim()) return `${refName(ref)}（引用）`;
  const base = typeText(schema);
  if (base !== 'array') return base;
  const items = schema.items;
  if (isObject(items)) return `array<${typeText(items)}>`;
  if (Array.isArray(items)) {
    // `items` 是一组候选时取第一条的类型：其余候选之间的差别要靠整份文档才能说清。
    const first = items.find((item) => isObject(item));
    return `array<${first ? typeText(first) : 'any'}>`;
  }
  return 'array';
}

/**
 * 组合关键字（`oneOf` / `anyOf` / `allOf`）的一句话说明。
 *
 * 这三种写法都只给一句话，不去合并候选：候选之间的差别要靠整份文档才能说清，
 * 硬合并出来的类型反而会让用户以为那份定义只有一个形状。
 */
function combinatorNote(schema: SchemaObject): string | null {
  if (Array.isArray(schema.oneOf)) return `候选之一：${describeCandidates(schema.oneOf)}`;
  if (Array.isArray(schema.anyOf)) return `任一满足：${describeCandidates(schema.anyOf)}`;
  if (Array.isArray(schema.allOf)) return `同时满足：${describeCandidates(schema.allOf)}`;
  return null;
}

/** 候选写法的短描述：拿得到名字就列名字，拿不到就报个数。 */
function describeCandidates(candidates: unknown[]): string {
  const names = candidates
    .filter((candidate): candidate is SchemaObject => isObject(candidate))
    .map((candidate) =>
      typeof candidate.$ref === 'string' && candidate.$ref.trim()
        ? refName(candidate.$ref)
        : typeText(candidate),
    );
  if (!names.length) return `${candidates.length} 种写法`;
  if (names.length > 4) return `${names.slice(0, 4).join(' | ')} 等 ${names.length} 种写法`;
  return names.join(' | ');
}

/** 父级 `required` 列表里的字段名。 */
function requiredNames(schema: SchemaObject): Set<string> {
  const required = schema.required;
  if (!Array.isArray(required)) return new Set();
  return new Set(required.filter((name): name is string => typeof name === 'string'));
}

/** 对象的属性列表，转成字段数组。 */
function fieldsOf(schema: SchemaObject): SchemaField[] {
  const properties = schema.properties;
  if (!isObject(properties)) return [];
  const required = requiredNames(schema);
  return Object.entries(properties).map(([name, value]) =>
    buildField(name, value, required.has(name)),
  );
}

/** 一个属性的展示形状。 */
function buildField(name: string, raw: unknown, required: boolean): SchemaField {
  // 属性值不是对象（例如写成了一个字符串）：读不出类型，只显示名字。
  if (!isObject(raw)) return { name, type: 'any', required };

  const field: SchemaField = { name, type: displayType(raw), required };

  const description = raw.description;
  if (typeof description === 'string' && description.trim()) field.description = description.trim();

  const format = raw.format;
  if (typeof format === 'string' && format.trim()) field.format = format.trim();

  if (Array.isArray(raw.enum) && raw.enum.length) {
    field.enumValues = raw.enum.map((value) => valueText(value));
  }
  if ('default' in raw) field.defaultValue = valueText(raw.default);

  const notes: string[] = [];
  const combinator = combinatorNote(raw);
  if (combinator) notes.push(combinator);
  if (Array.isArray(raw.items) && raw.items.length > 1) {
    notes.push(`元素有 ${raw.items.length} 种候选写法`);
  }
  if (notes.length) field.note = notes.join('；');

  // 嵌套字段：对象的子属性，或数组元素的属性。
  const children = isObject(raw.properties)
    ? fieldsOf(raw)
    : isObject(raw.items)
      ? fieldsOf(raw.items)
      : [];
  if (children.length) field.children = children;

  return field;
}

/** 枚举值与默认值的文本：字符串带引号，其余按 JSON 的写法。 */
function valueText(value: unknown): string {
  if (typeof value === 'string') return JSON.stringify(value);
  if (value === undefined) return 'undefined';
  try {
    const text = JSON.stringify(value);
    return text === undefined ? String(value) : text;
  } catch {
    return String(value);
  }
}

/**
 * 把一段 Schema 文本解析成可以画成表格的结构。
 *
 * 空文本不算失败：它表示「还没写」，界面据此给出空态提示。
 */
export function parseSchemaView(text: string): SchemaViewResult {
  const trimmed = text.trim();
  if (!trimmed) return { ok: true, view: { type: '', fields: [], empty: true } };

  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch (error) {
    return { ok: false, kind: 'parse-error', message: (error as Error).message };
  }

  if (!isObject(parsed)) {
    return {
      ok: false,
      kind: 'not-an-object',
      message: `这份定义的顶层是${jsonTypeName(parsed)}，不是一个对象`,
    };
  }

  const view: SchemaView = { type: displayType(parsed), fields: [], empty: false };

  const description = parsed.description;
  if (typeof description === 'string' && description.trim()) view.description = description.trim();

  const rootNote = combinatorNote(parsed);
  if (rootNote) view.note = rootNote;

  if (isObject(parsed.items)) {
    // 根节点是数组时，元素的定义才是「有哪些字段」的答案。
    const itemFields = fieldsOf(parsed.items);
    if (itemFields.length) {
      view.fields = itemFields;
      const elementNote = '下列字段属于数组元素的定义';
      view.note = view.note ? `${view.note}；${elementNote}` : elementNote;
    }
  } else {
    view.fields = fieldsOf(parsed);
  }

  return { ok: true, view };
}
