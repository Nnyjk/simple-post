/**
 * GraphQL introspection 的解析与光标处补全。纯函数，不碰 store、不碰网络：调用方负责把
 * introspection 响应体交给 `parseIntrospection`，再把解析结果交给 `completionCandidates`。
 *
 * 补全按**文本上下文**判断，而不是做完整语法分析：用户在任何一次击键之后都可能停在半个
 * 查询里，解析器遇到不完整的文档只会报错，而这里要的是「此刻能写什么」。
 */

/** 类型目录里认得的种类。introspection 里的其它种类一律跳过。 */
export type GraphQlTypeKind = 'OBJECT' | 'INTERFACE' | 'INPUT_OBJECT' | 'ENUM' | 'SCALAR' | 'UNION';

/** 类型上的一个字段。`type` 是剥掉 `!` 与 `[]` 之后的类型名。 */
export interface GraphQlField {
  name: string;
  args: string[];
  type: string;
}

export interface GraphQlType {
  kind: GraphQlTypeKind;
  fields: GraphQlField[];
  enumValues: string[];
}

export interface GraphQlSchema {
  queryType: string | null;
  mutationType: string | null;
  subscriptionType: string | null;
  types: Record<string, GraphQlType>;
}

export interface GraphQlCompletion {
  label: string;
  detail: string;
}

/**
 * 标准 introspection 查询。只取补全用得到的部分：根类型名、类型的种类与字段、字段参数名、
 * 枚举值与字段的返回类型。
 *
 * `ofType` 只展开三层：`[User!]!` 的链条是 NON_NULL → LIST → NON_NULL → OBJECT，从 `type`
 * 算起正好四层，够剥到实际类型名。再深的包装（列表套列表）在返回类型名上等价，多取也只是
 * 多花字节。
 */
export const INTROSPECTION_QUERY = `query IntrospectionQuery {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types {
      kind
      name
      fields {
        name
        args { name }
        type {
          kind
          name
          ofType {
            kind
            name
            ofType {
              kind
              name
              ofType { kind name }
            }
          }
        }
      }
      enumValues { name }
    }
  }
}`;

/** 类型目录里认得的种类表；`kind` 落在表外说明样本来自别的 GraphQL 实现，跳过该类型。 */
const TYPE_KIND_FLAGS: Record<GraphQlTypeKind, true> = {
  OBJECT: true,
  INTERFACE: true,
  INPUT_OBJECT: true,
  ENUM: true,
  SCALAR: true,
  UNION: true,
};

/** 有子字段、能直接挂选择集的种类。 */
const COMPOSITE_KIND_FLAGS: Partial<Record<GraphQlTypeKind, true>> = {
  OBJECT: true,
  INTERFACE: true,
};

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function isTypeKind(value: string): value is GraphQlTypeKind {
  return Object.prototype.hasOwnProperty.call(TYPE_KIND_FLAGS, value);
}

/** 剥掉 `!` 与 `[]` 包装后的类型名。`ref` 是 introspection 的 `type` 对象。 */
function unwrapTypeName(ref: unknown): string {
  let current = ref;
  let name = '';
  while (current !== null && typeof current === 'object') {
    const record = current as Record<string, unknown>;
    if (typeof record.name === 'string' && record.name) name = record.name;
    current = record.ofType;
  }
  return name;
}

function parseField(raw: unknown): GraphQlField | null {
  const record = asRecord(raw);
  if (!record || typeof record.name !== 'string' || !record.name) return null;
  const args = Array.isArray(record.args)
    ? record.args
        .map((arg) => asRecord(arg))
        .filter((arg): arg is Record<string, unknown> => arg !== null)
        .map((arg) => arg.name)
        .filter((name): name is string => typeof name === 'string' && name !== '')
    : [];
  return { name: record.name, args, type: unwrapTypeName(record.type) };
}

function parseEnumValues(raw: unknown): string[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .map((entry) => asRecord(entry))
    .filter((entry): entry is Record<string, unknown> => entry !== null)
    .map((entry) => entry.name)
    .filter((name): name is string => typeof name === 'string' && name !== '');
}

function rootTypeName(raw: unknown): string | null {
  const record = asRecord(raw);
  return record && typeof record.name === 'string' && record.name ? record.name : null;
}

/**
 * 解析 introspection 响应体（已经 `JSON.parse` 过的对象）。形状不对时返回 `null`，
 * **不抛**：样本可以来自任何地址，用户贴进来一个 404 页面不该让编辑器炸掉。
 */
export function parseIntrospection(body: unknown): GraphQlSchema | null {
  const root = asRecord(body);
  const data = root ? asRecord(root.data) : null;
  const schema = data ? asRecord(data.__schema) : null;
  if (!schema || !Array.isArray(schema.types)) return null;

  const types: Record<string, GraphQlType> = {};
  for (const rawType of schema.types) {
    const record = asRecord(rawType);
    if (!record) return null;
    const { kind, name } = record;
    if (typeof kind !== 'string' || typeof name !== 'string' || !name) return null;
    // 内省类型（`__Type` 之类）不是用户能写进查询的东西。
    if (name.startsWith('__')) continue;
    if (!isTypeKind(kind)) continue;
    types[name] = {
      kind,
      fields: Array.isArray(record.fields)
        ? record.fields.map(parseField).filter((field): field is GraphQlField => field !== null)
        : [],
      enumValues: parseEnumValues(record.enumValues),
    };
  }

  return {
    queryType: rootTypeName(schema.queryType),
    mutationType: rootTypeName(schema.mutationType),
    subscriptionType: rootTypeName(schema.subscriptionType),
    types,
  };
}

interface ScanLevel {
  /** 这一层选择集所属的类型；`null` 表示不知道，补全就不猜。 */
  type: string | null;
  /** 这一层最近读到的字段名，后面跟 `{` 时用它算子层的类型。 */
  lastField: string | null;
}

interface ScanParen {
  /** 括号所属的字段名；变量定义括号为 `null`。 */
  fieldName: string | null;
  /** 变量定义括号（`query Q($x: Int)`）。 */
  isVariables: boolean;
}

interface ScanState {
  levels: ScanLevel[];
  parens: ScanParen[];
  rootKeyword: 'query' | 'mutation' | 'subscription';
  /** `... on Type` / `fragment F on Type` 里读到的类型名，紧跟的 `{` 用它。 */
  pendingType: string | null;
  /** 上一条有意义的记号，用来识别 `on`、`$`、`@` 之后的标识符。 */
  previous: string | null;
  /** 扫描到光标时是否仍在字符串或注释里 —— 那时补全没有意义。 */
  inString: boolean;
  inComment: boolean;
}

function isIdentStart(ch: string): boolean {
  return /[A-Za-z_]/.test(ch);
}

function isIdentPart(ch: string): boolean {
  return /[A-Za-z0-9_]/.test(ch);
}

/** 当前层所属的类型名：根层用文档里出现过的操作关键字决定。 */
function typeOfField(schema: GraphQlSchema, parent: string | null, field: string): string | null {
  if (!parent) return null;
  const type = schema.types[parent];
  if (!type) return null;
  return type.fields.find((entry) => entry.name === field)?.type ?? null;
}

function rootTypeOf(schema: GraphQlSchema, keyword: ScanState['rootKeyword']): string | null {
  return keyword === 'mutation'
    ? schema.mutationType
    : keyword === 'subscription'
      ? schema.subscriptionType
      : schema.queryType;
}

/** 不是字段名的关键字。`on` 与 `$`/`@` 之后的标识符另有处理，见 `scanBefore`。 */
const KEYWORDS: Record<string, true> = {
  query: true,
  mutation: true,
  subscription: true,
  fragment: true,
  on: true,
  true: true,
  false: true,
  null: true,
  directive: true,
  repeatable: true,
};

/**
 * 扫过 `document` 里光标之前的字符，维护「当前在哪一层选择集、括号里、还是字符串里」。
 * 字符串与注释的内容被跳过：查询里的 `{` 有一半概率出现在字符串里，按字面计入会把上下文
 * 算歪。
 */
function scanBefore(document: string, position: number, schema: GraphQlSchema): ScanState {
  const state: ScanState = {
    levels: [{ type: null, lastField: null }],
    parens: [],
    rootKeyword: 'query',
    pendingType: null,
    previous: null,
    inString: false,
    inComment: false,
  };
  const limit = Math.min(position, document.length);
  let i = 0;
  while (i < limit) {
    const ch = document[i]!;

    if (state.inComment) {
      if (ch === '\n') state.inComment = false;
      i += 1;
      continue;
    }

    if (state.inString) {
      if (ch === '\\' && document[i + 1] === '"') {
        i += 2;
        continue;
      }
      if (document.startsWith('"""', i)) {
        state.inString = false;
        i += 3;
        continue;
      }
      if (ch === '"' && !document.startsWith('"""', i)) {
        state.inString = false;
      }
      i += 1;
      continue;
    }

    if (ch === '#') {
      state.inComment = true;
      i += 1;
      continue;
    }

    if (ch === '"') {
      state.inString = true;
      // 块字符串以三个引号开头；三个引号内部允许再出现单个引号。
      i += document.startsWith('"""', i) ? 3 : 1;
      state.previous = 'string';
      continue;
    }

    if (ch === '{') {
      const level = state.levels[state.levels.length - 1]!;
      const opened =
        state.pendingType ??
        (level.lastField ? typeOfField(schema, level.type, level.lastField) : null) ??
        (state.levels.length === 1 ? rootTypeOf(schema, state.rootKeyword) : null);
      state.levels.push({ type: opened, lastField: null });
      state.pendingType = null;
      state.previous = '{';
      i += 1;
      continue;
    }

    if (ch === '}') {
      if (state.levels.length > 1) state.levels.pop();
      state.previous = '}';
      i += 1;
      continue;
    }

    if (ch === '(') {
      const level = state.levels[state.levels.length - 1]!;
      // 变量定义括号长在操作声明上：此时还在根层，且括号里写的是 `$name: Type`。
      const isVariables = state.levels.length === 1 && level.lastField === null;
      state.parens.push({
        fieldName: isVariables ? null : level.lastField,
        isVariables,
      });
      state.previous = '(';
      i += 1;
      continue;
    }

    if (ch === ')') {
      state.parens.pop();
      state.previous = ')';
      i += 1;
      continue;
    }

    if (ch === '$' || ch === '@' || ch === '.' || ch === ',' || ch === ':') {
      state.previous = ch;
      i += 1;
      continue;
    }

    if (ch === '[' || ch === ']' || ch === '!') {
      state.previous = ch;
      i += 1;
      continue;
    }

    if (isIdentStart(ch)) {
      let end = i + 1;
      while (end < limit && isIdentPart(document[end]!)) end += 1;
      const word = document.slice(i, end);
      const previous = state.previous;

      if (previous === 'on' || previous === '...') {
        // `... on Type` 与 `fragment F on Type` 的 Type 位置。
        state.pendingType = word;
      } else if (previous === '$' || previous === '@') {
        // 变量名与指令名不是字段。
      } else if (KEYWORDS[word]) {
        if (state.levels.length === 1 && state.parens.length === 0 && word !== 'fragment') {
          if (word === 'query' || word === 'mutation' || word === 'subscription') {
            state.rootKeyword = word;
          }
        }
      } else if (state.parens.length === 0) {
        const level = state.levels[state.levels.length - 1]!;
        if (state.levels.length > 1) level.lastField = word;
      }

      state.previous = word;
      i = end;
      continue;
    }

    i += 1;
  }
  return state;
}

/** 光标前的文本，字符串与注释的内容换成空格：括号计数只看代码本身。 */
function cleanTextBefore(document: string, position: number): string {
  return document
    .slice(0, Math.min(position, document.length))
    .replace(/"""[\s\S]*?"""|"(?:\\.|[^"\\])*"|#[^\n]*/g, (match) => ' '.repeat(match.length));
}

/**
 * 光标处的补全候选。
 *
 * 只看光标之前：之后的文本还是用户没写完的意图，掺进来只会把上下文算歪。匹配不上任何已知
 * 上下文时返回空数组 —— 猜一个错误的候选列表比不给更让人困惑。
 */
export function completionCandidates(
  document: string,
  position: number,
  schema: GraphQlSchema,
): GraphQlCompletion[] {
  const state = scanBefore(document, position, schema);
  if (state.inString || state.inComment) return [];

  const text = cleanTextBefore(document, position);

  const typeCandidates = (flags: Partial<Record<GraphQlTypeKind, true>>): GraphQlCompletion[] =>
    Object.entries(schema.types)
      .filter(([, type]) => flags[type.kind] === true)
      .map(([name, type]) => ({ label: name, detail: `${type.kind} ${name}` }))
      .sort((a, b) => a.label.localeCompare(b.label));

  // `... on <此处>`：联合类型与接口的成员只能这么写。
  if (/\.\.\.\s*on\s*[\w_]*$/.test(text) || /\bfragment\s+[\w_]+\s+on\s+[\w_]*$/.test(text)) {
    return typeCandidates(COMPOSITE_KIND_FLAGS);
  }

  // `$var: <此处>`：变量定义的类型位置，接受任何具名类型。
  if (/\$[\w_]*\s*:\s*[\w_[\]!]*$/.test(text)) {
    return typeCandidates(TYPE_KIND_FLAGS);
  }

  // 参数括号里：内省查询只取了参数名，故 detail 明说是参数而不是类型。
  const paren = state.parens[state.parens.length - 1];
  if (paren) {
    if (paren.isVariables || !paren.fieldName) return [];
    const level = state.levels[state.levels.length - 1]!;
    const type = level.type ? schema.types[level.type] : undefined;
    const field = type?.fields.find((entry) => entry.name === paren.fieldName);
    return (field?.args ?? [])
      .map((name) => ({ label: name, detail: '参数' }))
      .sort((a, b) => a.label.localeCompare(b.label));
  }

  // 选择集里：列当前类型的字段。根选择集的类型由 `scanBefore` 在 `{` 处按操作关键字定好，
  // 这里不再区分根与内层；光标落在文档层（没进任何选择集）时类型为 null，不给候选。
  const level = state.levels[state.levels.length - 1]!;
  if (!level.type) return [];
  const type = schema.types[level.type];
  // 联合类型没有共同字段（要写 `... on`），标量与枚举没有子字段。
  if (!type || COMPOSITE_KIND_FLAGS[type.kind] !== true) return [];
  return type.fields
    .map((field) => ({
      label: field.name,
      detail: `${schema.types[field.type]?.kind ?? 'OBJECT'} ${field.type}`,
    }))
    .sort((a, b) => a.label.localeCompare(b.label));
}
