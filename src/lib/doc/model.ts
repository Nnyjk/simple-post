/**
 * 文档模型 —— 导出那份 HTML 之前的中间层。
 *
 * 它把 store 里**已保存**的 `nodes` 与 `endpointDetails` 折成一份纯数据（自包含、无函数、无 DOM），
 * 渲染层只认这份数据，不再回头摸 store。分这一层的理由有三条：
 *
 *   1. 渲染因此是纯函数：同一份模型 + 同一个 `generatedAt` 必得同一份字节，测试与复现都靠它。
 *   2. **「不写值」这条硬约束落在这里。** 组装模型时只读 `key` 与 `description`，`value` 与
 *      `enabled` 之外的字段一概不碰——渲染层就算想泄漏也没有数据可泄漏。
 *   3. 分组、排序、内容类型这些决策只写一遍，测试直接对着模型断言，不必解析 HTML。
 *
 * 排序规则与工作区树一致：先 `sortOrder`，再按 id 的码位比较（不用 `localeCompare`，它随环境
 * 语言设置变，会破坏「同一输入同一输出」）。
 */

import type {
  AuthConfig,
  BodyMode,
  EndpointDetail,
  Environment,
  KeyValue,
  ResourceNode,
  ResponseDefinition,
} from '@/types/domain';
import type { TransferNode } from '@/lib/interchange-types';

/** 表格里的一行：只有名字与说明，**没有值**。 */
export interface DocField {
  name: string;
  description: string;
}

/** 请求体摘要。`schema` 是原始文本，渲染时才规范化。 */
export interface DocBody {
  mode: BodyMode;
  contentType: string;
  schema?: string;
}

/** 一条响应定义。`example` 是用户专门写作文档的正文，允许原样输出。 */
export interface DocResponse {
  status: string;
  description: string;
  contentType: string;
  schema?: Record<string, unknown>;
  example?: string;
}

/** 鉴权摘要：只写机制与**名字**，不写任何凭据。 */
export interface DocAuth {
  kind: AuthConfig['kind'];
  /** 补充说明（apikey 的位置与参数名、oauth2 的授权类型）。已含中文，为空表示无需补充。 */
  detail?: string;
}

export interface DocEndpoint {
  id: string;
  name: string;
  /** 接口说明（节点上那句一句话描述）。 */
  description: string;
  method: string;
  /** URL 模板，保留 `{{var}}` / `:id` 之类的占位符——文档本来就不该把变量解析成值。 */
  url: string;
  /** 长篇文档（Markdown 原文）。 */
  docs?: string;
  /** 备注（Markdown 原文）。 */
  notes?: string;
  pathParams: DocField[];
  params: DocField[];
  headers: DocField[];
  auth: DocAuth;
  body?: DocBody;
  responses: DocResponse[];
}

export interface DocGroup {
  id: string;
  name: string;
  endpoints: DocEndpoint[];
  children: DocGroup[];
}

/** 环境变量的**名字**清单——只有名字，`variables` 的值一个都不读。 */
export interface DocEnvironment {
  name: string;
  variables: Array<{ name: string; disabled: boolean }>;
}

export interface ApiDocModel {
  projectName: string;
  /** 生成本份文档的模块名（导出入口按模块选取）。 */
  moduleName: string;
  /** 展示用时间，`YYYY-MM-DD HH:mm`，本地时区。 */
  generatedAt: string;
  /** `generatedAt` 的 ISO 形式，供 `<time datetime>`。 */
  generatedAtIso: string;
  endpointCount: number;
  environment: DocEnvironment | null;
  /** 模块本身即根分组：它的直接子接口排在孩子们前面。 */
  root: DocGroup;
}

/** 组装模型需要的输入：全部来自已保存状态，加上调用方给的时刻。 */
export interface DocSource {
  projectName: string;
  moduleId: string;
  nodes: readonly ResourceNode[];
  details: Readonly<Record<string, EndpointDetail>>;
  /** 选中的接口节点 id。不在集合里的接口不进文档。 */
  selected: ReadonlySet<string>;
  /** 由调用方传入：渲染本身不许读时钟，否则同一输入得不到同一字节。 */
  generatedAt: Date;
  environment?: Environment | null;
}

/** `YYYY-MM-DD HH:mm`（本地时区）。手写而不用 `toLocaleString`，后者随运行环境变格式。 */
export function formatDocTimestamp(date: Date): string {
  return `${pad(date.getFullYear(), 4)}-${pad(date.getMonth() + 1, 2)}-${pad(date.getDate(), 2)} ${pad(
    date.getHours(),
    2,
  )}:${pad(date.getMinutes(), 2)}`;
}

/** `YYYYMMDD`（本地时区），文件名用。 */
export function formatDocDateStamp(date: Date): string {
  return `${pad(date.getFullYear(), 4)}${pad(date.getMonth() + 1, 2)}${pad(date.getDate(), 2)}`;
}

function pad(value: number, width: number): string {
  return String(value).padStart(width, '0');
}

/**
 * 工作区里 `moduleId` 子树的可选节点树，形如导入/导出对话框里的 `TransferNode`（同一套选择逻辑
 * 才复用得上）。根节点就是模块本身，`endpointTotal` 是那棵子树的接口总数。
 *
 * 接口行上的 METHOD 与路径取自**已保存**的 `details`：对话框要显示用户认得的那两样东西，
 * 而不是节点 id。
 */
export function docGroups(
  nodes: readonly ResourceNode[],
  details: Readonly<Record<string, EndpointDetail>>,
  moduleId: string,
): TransferNode[] {
  const root = nodes.find((n) => n.id === moduleId && n.kind === 'module');
  if (!root) return [];
  const children = childrenIndex(nodes);
  return [toTransferNode(root, children, details)];
}

function toTransferNode(
  node: ResourceNode,
  children: ChildrenIndex,
  details: Readonly<Record<string, EndpointDetail>>,
): TransferNode {
  const kids = children.get(node.id) ?? [];
  const endpoints = kids
    // 没有 `detail` 的节点列表里给不出 METHOD 与路径，也进不了文档（`buildGroup` 会跳过）。
    // 留在这里只会让「已选 n 个」与实际导出的数量对不上。
    .filter((n) => n.kind === 'endpoint' && details[n.id] !== undefined)
    .sort(bySortOrder)
    .map((n) => {
      const detail = details[n.id];
      return {
        key: n.id,
        name: n.name,
        method: detail?.method ?? 'GET',
        path: detail?.url ?? '',
        bodyMode: detail?.body.mode ?? 'none',
        auth: detail?.auth.kind ?? 'none',
        duplicate: false,
        diagnosticIndexes: [] as number[],
      };
    });
  const groups = kids
    .filter((n) => n.kind === 'collection')
    .sort(bySortOrder)
    .map((n) => toTransferNode(n, children, details));
  const total = endpoints.length + groups.reduce((sum, g) => sum + g.endpointTotal, 0);
  return { key: node.id, name: node.name, endpoints, children: groups, endpointTotal: total };
}

type ChildrenIndex = Map<string, ResourceNode[]>;

function childrenIndex(nodes: readonly ResourceNode[]): ChildrenIndex {
  const index: ChildrenIndex = new Map();
  for (const node of nodes) {
    const parent = node.parentId;
    if (!parent) continue;
    const list = index.get(parent);
    if (list) list.push(node);
    else index.set(parent, [node]);
  }
  return index;
}

function bySortOrder(a: ResourceNode, b: ResourceNode): number {
  if (a.sortOrder !== b.sortOrder) return a.sortOrder - b.sortOrder;
  // 同序时按码位比 id：不用 localeCompare，它随运行环境的语言设置变，会破坏「同一输入同一输出」。
  if (a.id === b.id) return 0;
  return a.id < b.id ? -1 : 1;
}

/** 把已保存的定义折成文档模型。纯函数：不读时钟、不读 store、不改入参。 */
export function buildApiDocModel(src: DocSource): ApiDocModel {
  const root = src.nodes.find((n) => n.id === src.moduleId && n.kind === 'module');
  const children = childrenIndex(src.nodes);

  let endpointCount = 0;
  const seen = new Set<string>();

  const buildGroup = (node: ResourceNode): DocGroup => {
    const kids = children.get(node.id) ?? [];
    const endpoints: DocEndpoint[] = [];
    for (const kid of kids.filter((n) => n.kind === 'endpoint').sort(bySortOrder)) {
      if (!src.selected.has(kid.id) || seen.has(kid.id)) continue;
      const detail = src.details[kid.id];
      if (!detail) continue;
      seen.add(kid.id);
      endpoints.push(toDocEndpoint(kid, detail));
      endpointCount += 1;
    }
    const groups = kids
      .filter((n) => n.kind === 'collection')
      .sort(bySortOrder)
      .map(buildGroup);
    return { id: node.id, name: node.name, endpoints, children: groups };
  };

  const docRoot: DocGroup = root
    ? buildGroup(root)
    : { id: src.moduleId, name: '', endpoints: [], children: [] };

  return {
    projectName: src.projectName,
    moduleName: docRoot.name,
    generatedAt: formatDocTimestamp(src.generatedAt),
    generatedAtIso: src.generatedAt.toISOString(),
    endpointCount,
    environment: src.environment ? toDocEnvironment(src.environment) : null,
    root: docRoot,
  };
}

function toDocEnvironment(env: Environment): DocEnvironment {
  const names = Object.keys(env.variables).sort();
  const disabled = new Set(env.disabledVariables);
  return {
    name: env.name,
    variables: names.map((name) => ({ name, disabled: disabled.has(name) })),
  };
}

function toDocEndpoint(node: ResourceNode, detail: EndpointDetail): DocEndpoint {
  const body = toDocBody(detail);
  return {
    id: node.id,
    name: node.name,
    description: node.description?.trim() ?? '',
    method: detail.method,
    url: detail.url,
    docs: trimOrUndefined(detail.docs),
    notes: trimOrUndefined(detail.notes),
    pathParams: toDocFields(detail.pathParams),
    params: toDocFields(detail.params),
    headers: toDocFields(detail.headers),
    auth: toDocAuth(detail.auth),
    body,
    responses: detail.responses.map(toDocResponse),
  };
}

/**
 * 只留名字与说明。`value` 在这里被彻底丢弃——它是整份文档唯一可能带凭据的字段，也正因为它在
 * 这一层就被扔了，后面任何渲染改动都不可能把它带回来。
 */
function toDocFields(items: readonly KeyValue[] | undefined): DocField[] {
  const out: DocField[] = [];
  for (const item of items ?? []) {
    if (!item.enabled || item.key.trim() === '') continue;
    out.push({ name: item.key, description: item.description?.trim() ?? '' });
  }
  return out;
}

function toDocAuth(auth: AuthConfig): DocAuth {
  switch (auth.kind) {
    case 'apikey': {
      const where = auth.apikey?.inLocation === 'query' ? '查询参数' : '请求头';
      const key = auth.apikey?.key?.trim();
      return { kind: auth.kind, detail: key ? `${where}「${key}」，值不随文档输出` : undefined };
    }
    case 'oauth2':
      return { kind: auth.kind, detail: auth.oauth2?.grantType };
    default:
      return { kind: auth.kind };
  }
}

function toDocBody(detail: EndpointDetail): DocBody | undefined {
  const mode = detail.body?.mode ?? 'none';
  if (mode === 'none') return undefined;
  return {
    mode,
    contentType: contentTypes[mode] ?? 'application/octet-stream',
    schema: trimOrUndefined(detail.body.schema),
  };
}

/** 与 `src/lib/http-build/headers.ts` 的自动 Content-Type 保持一致（那份函数是私有的，这里只能抄规则）。 */
const contentTypes: Partial<Record<BodyMode, string>> = {
  json: 'application/json',
  form: 'application/x-www-form-urlencoded',
  xml: 'application/xml',
  raw: 'text/plain',
  multipart: 'multipart/form-data（boundary 每次请求现生成）',
  binary: '文件自身的 MIME 类型，缺省 application/octet-stream',
};

function toDocResponse(res: ResponseDefinition): DocResponse {
  return {
    status: res.status,
    description: res.description,
    contentType: res.contentType,
    schema: res.schema,
    example: trimOrUndefined(res.example ?? undefined),
  };
}

function trimOrUndefined(value: string | undefined): string | undefined {
  const text = value?.trim();
  return text ? text : undefined;
}
