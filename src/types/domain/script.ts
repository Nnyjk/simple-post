/**
 * 一个节点上的两段脚本。collection 的脚本住这里；端点的脚本在
 * `EndpointDetail` 上（与 body / headers 同级）。
 */
export interface NodeScripts {
  nodeId: string;
  /** 请求发出之前跑的那段。`null`（或缺省）= 没有脚本。 */
  preRequestScript?: string | null;
  /** 响应回来之后跑的那段。`null`（或缺省）= 没有脚本。 */
  testScript?: string | null;
}

/** 脚本在哪个阶段跑。 */
export type ScriptPhase = 'pre' | 'test';

/** 脚本失败的原因分类。 */
export type ScriptErrorKind = 'compile' | 'runtime' | 'timeout' | 'memory' | 'stack';

/** 一条脚本日志。`level` 是 log / info / warn / error。 */
export interface ScriptLog {
  level: string;
  message: string;
}

/** 一条命名断言的结果。 */
export interface AssertionResult {
  name: string;
  passed: boolean;
  /** 失败时的消息；通过时为 null。 */
  error?: string | null;
}

/** 一个脚本的失败。`source` 是出问题的那段脚本（collection 名 / 端点名）。 */
export interface ScriptError {
  kind: ScriptErrorKind;
  message: string;
  source: string;
  phase: ScriptPhase;
}

/** 一条环境变量写入。`value` 为 null 表示删除该变量。 */
export interface EnvWrite {
  key: string;
  value: string | null;
}

/**
 * 一次发送里脚本的全部产出。
 *
 * 脚本失败**不影响请求的成败**——错误在 `errors` 里单独呈现，请求的成败照旧。
 */
export interface ScriptOutcome {
  logs: ScriptLog[];
  assertions: AssertionResult[];
  errors: ScriptError[];
  /** 脚本写下的环境变量（已由宿主落库的那些）。 */
  envWrites: EnvWrite[];
}

/**
 * 脚本看得到的环境快照。
 *
 * `id` 决定 `pm.environment.set` 往哪写；`null` 表示没有环境，那种写入会被丢弃。
 */
export interface EnvSnapshot {
  id: string | null;
  variables: Record<string, string>;
}
