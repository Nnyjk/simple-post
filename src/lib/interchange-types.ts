/**
 * 导入 / 导出的线类型。与 `crates/simple-post-server/src/dto.rs` 里那几个结构体一一对应。
 *
 * **导入与导出共用同一棵 `TransferNode` 树**：勾选弹窗只有一个组件，一份类型才叫共用；
 * 两套形状在组件里各写一半再凑合渲染，那种「共用」只是文件名上的共用。
 */
import type { AuthType, BodyMode } from '@/types/domain';

/** 线上的诊断严重度。Rust 侧 `simple_post_interchange::Severity` 的小写形式。 */
export type Severity = 'info' | 'warn' | 'error';

/** 外来格式的诊断。`pointer` 是外来文档里该构造的位置，用户据此找得到它。 */
export interface TransferDiagnostic {
  severity: Severity;
  pointer: string;
  message: string;
  /** 没能落进去的那个构造本身。缺席表示整个条目被跳过。 */
  dropped?: string;
}

/** 勾选树上的一行接口。 */
export interface TransferEndpoint {
  /** 稳定标识。导入时是外来文档里的位置指针，导出时是 `nodes.id`。 */
  key: string;
  name: string;
  /** 大写动词。`TRACE` 之类不是 `HTTP_METHODS` 里的那七个。 */
  method: string;
  /** 只有路径。host 不在这里，它挂在分组那一层的 `servers` 上。 */
  path: string;
  bodyMode: BodyMode;
  auth: AuthType;
  /** 与项目里已有的某个接口指纹相同。默认仍然勾上，落库时会被跳过。 */
  duplicate: boolean;
  /** 落在这一行上的诊断在 `diagnostics` 里的下标。 */
  diagnosticIndexes: number[];
}

/** 勾选树上的一个分组（导入时是「将要创建成集合的那一层」，导出时是集合）。 */
export interface TransferNode {
  key: string;
  name: string;
  endpoints: TransferEndpoint[];
  children: TransferNode[];
  /** 本子树里的接口总数（含所有层级）。 */
  endpointTotal: number;
}

export interface TransferCounts {
  groups: number;
  endpoints: number;
  duplicates: number;
}

export interface ImportedEnvironment {
  name: string;
  variableCount: number;
  disabledVariableCount: number;
}

/**
 * 预览里「这份文档会创建成一个服务」的预告。
 *
 * **缺席是真实答案**，不是「待定」：文档没给 host、或给了多于一个 host 就是缺席
 * （后者附一条 `warn` 诊断说明为什么）。
 */
export interface ProposedService {
  /** 服务名，即 host。项目内唯一。 */
  service: string;
  /** 落库时写进目标模块那一格的 URL。 */
  url: string;
  /** 项目里已有同名服务，复用而不是新建。URL 不在服务行上，故复用不动别的模块的地址。 */
  existing: boolean;
  /** URL 会写进哪个环境。缺席 = 项目没有激活环境，服务仍然建，只是格子无处可填。 */
  environmentName?: string;
}

/** 落库报告里已经落下的服务。 */
export interface AppliedService {
  name: string;
  url: string;
  /** 服务定义是新建的；`false` 表示复用了项目里已有的同名服务。 */
  created: boolean;
  /** 端点有没有被指向它。为 `false` 只有一种情形：项目没有激活环境。 */
  assigned: boolean;
  /** URL 写进了哪个环境。 */
  environmentName?: string;
}

export interface ImportPreview {
  /** `postman-collection-v2.1` / `openapi-3`。给程序用的线上名。 */
  format: string;
  /** 人读的名字。 */
  formatLabel: string;
  title: string | null;
  sourceName: string;
  groups: TransferNode[];
  counts: TransferCounts;
  diagnostics: TransferDiagnostic[];
  environments: ImportedEnvironment[];
  service?: ProposedService;
  /** 仅 URL 模式回传：拉到的原文。提交时原样送回，故一次导入只拉一次。 */
  resolvedContent?: string;
}

export interface ImportCommitResult {
  createdEndpoints: number;
  skippedDuplicates: number;
  createdCollectionIds: string[];
  createdEndpointIds: string[];
  environmentsCreated: string[];
  service?: AppliedService;
  diagnostics: TransferDiagnostic[];
}

export interface ModuleServers {
  moduleKey: string;
  moduleName: string;
  /** 空数组是真实答案：该模块在这个环境下没有配 URL，导出时不写 `servers`。 */
  servers: { service: string; url: string }[];
}

export interface ExportPreview {
  groups: TransferNode[];
  counts: TransferCounts;
  environmentId?: string;
  servers: ModuleServers[];
  diagnostics: TransferDiagnostic[];
}

export interface ExportDocument {
  fileName: string;
  mediaType: string;
  content: string;
  diagnostics: TransferDiagnostic[];
}

export type DocFormat = 'json' | 'yaml';

export const DOC_FORMATS: { value: DocFormat; label: string; extension: string }[] = [
  { value: 'json', label: 'JSON', extension: 'json' },
  { value: 'yaml', label: 'YAML', extension: 'yaml' },
];

/**
 * 严重度 → 徽标变体。
 *
 * `info` 用 muted 而不是 outline：它是「按设计如此 / 已在别处说明」，不该跟真正的问题
 * 抢注意力——这个区分与 Rust 侧 `CommitReport::notable_diagnostics` 滤掉 Info 是同一件事。
 */
export const SEVERITY_VARIANT: Record<Severity, 'muted' | 'warning' | 'destructive'> = {
  info: 'muted',
  warn: 'warning',
  error: 'destructive',
};

export const SEVERITY_LABEL: Record<Severity, string> = {
  info: '提示',
  warn: '警告',
  error: '错误',
};
