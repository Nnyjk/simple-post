/**
 * 批量运行里「发一条」的那一半 —— 取要素、组装、发送、记历史。
 *
 * 要素的推导与组装都走 `src/features/http-tester/model/endpointParts.ts`，那一份与
 * 单次发送共用，故「批量运行的变量替换语义与单次发送完全一致」是结构保证：两边跑
 * 的是同一个 `prepareRequest`，不是两套碰巧对上的实现。
 *
 * 脚本链由后端装配：发出去时带上端点 id，后端就会把祖先集合与端点自己的那两段脚本
 * 串起来（见 `core::script::chain`）。前端读不到集合脚本，也不需要读。
 */
import { sendPreparedRequest } from '@/lib/http';
import { validateRequest } from '@/lib/request-validation';
import { moduleIdOfEndpoint } from '@/lib/tree-index';
import {
  buildEndpointRequestParts,
  prepareEndpointRequest,
} from '@/features/http-tester/model/endpointParts';
import { toRequestSnapshot } from '@/features/http-tester/model/requestInput';
import type { RequestSettings } from '@/stores/app-store';
import type {
  ApiResponse,
  EndpointDetail,
  Environment,
  ResourceNode,
  StoredRequestSnapshot,
} from '@/types/domain';
import type { BatchItem, BatchOutcome } from './types';

/** 发一条端点需要的全部外部依赖。 */
export interface BatchSendContext {
  /** 工作区的全部节点（打平的数组）。 */
  nodes: readonly ResourceNode[];
  /** 端点的请求配置，按节点 id 索引。 */
  endpointDetails: Record<string, EndpointDetail>;
  /** 全部环境。**必须是批次开始时的快照**，见 {@link freezeEnvironments}。 */
  environments: readonly Environment[];
  /** 工作区当前激活的环境 id。端点不再自绑环境，故整批读这一个环境。 */
  activeEnvironmentId: string;
  /** 工作区当前项目 id。动态变量按项目解析，故要它。 */
  activeProjectId: string;
  /** 出站旋钮（超时、跟随重定向、响应体积上限、全局 TLS 设置）。 */
  settings: RequestSettings;
  /** 当前项目里已启用且仍存在的动态变量名。 */
  enabledDynamicNames: ReadonlySet<string>;
  /** 解析动态变量。 */
  resolveDynamicVariables: (
    names: string[],
    projectId?: string,
    force?: boolean,
  ) => Promise<{ values: Record<string, string>; errors: unknown[] }>;
  /** 把响应记进历史面板。 */
  addResponseHistory: (
    endpointId: string,
    response: ApiResponse,
    request?: StoredRequestSnapshot,
  ) => void;
}

/**
 * 拍一份环境的快照。
 *
 * 脚本可以把变量写回环境。若批量运行读的是 store 里的环境对象，那么第 1 条写回的
 * 变量会出现在第 2 条的替换里，「这一批看到的是同一份变量」就不成立了。故批次开始
 * 时把环境复制一份，整批都用这份副本：写回仍然落库（后端做），但**不影响**这一批
 * 后面那几条看到的值。
 */
export function freezeEnvironments(envs: readonly Environment[]): Environment[] {
  return envs.map((env) => ({
    ...env,
    variables: { ...env.variables },
    disabledVariables: [...(env.disabledVariables ?? [])],
    moduleBaseUrls: Object.fromEntries(
      Object.entries(env.moduleBaseUrls ?? {}).map(([moduleId, cell]) => [moduleId, { ...cell }]),
    ),
  }));
}

/**
 * 发一条端点，返回它的结论。
 *
 * 绝不抛：一条失败只是这一条的结论，整批照跑。
 */
export async function sendBatchItem(
  item: BatchItem,
  ctx: BatchSendContext,
  signal: AbortSignal,
): Promise<BatchOutcome> {
  const detail = ctx.endpointDetails[item.endpointId];
  if (!detail) {
    return failed(item, 'blocked', '这条接口没有请求配置');
  }

  // 环境：工作区**当前环境**。端点不再自绑环境，故这一批与单次发送读的是同一个环境
  // —— `useEndpointContext` 读的是 `useActiveEnvironment()`，那是同一个 store 状态。
  // 当前环境为空（还没有环境）时就没有环境：没有 base URL、变量也不替换，与单次发送
  // 在同一种情况下给出的请求一致。
  const env = ctx.environments.find((e) => e.id === ctx.activeEnvironmentId);

  const node = ctx.nodes.find((n) => n.id === item.endpointId);
  const moduleId = moduleIdOfEndpoint(ctx.nodes, item.endpointId, node);

  const parts = buildEndpointRequestParts({
    detail,
    // 批量运行跑的是**已存**的接口：未保存的草稿只存在于某个打开的标签页里，
    // 不是一个集合的成员。
    draft: undefined,
    endpointEnv: env,
    moduleId,
    enabledDynamicNames: ctx.enabledDynamicNames,
  });

  // body 解析不了就不发：照发只会换一个服务端解析错误，而这个错误本可以指出用户
  // 敲错的那个字符。与单次发送同一道门。
  const issue = validateRequest({ body: parts.effective.body });
  if (issue) {
    return failed(item, 'blocked', `请求体不是合法的 JSON：${issue.message}`);
  }

  // 动态变量的值必须在**替换之前**到手：替换一旦发生，变量池里就没有同名变量的
  // 概念了，而「动态变量压过同名环境变量」这条规则正是在替换之前生效的。
  let dynamicVars: Record<string, string> = {};
  if (parts.dynamicNames.length > 0) {
    const { values } = await ctx.resolveDynamicVariables(parts.dynamicNames, ctx.activeProjectId);
    dynamicVars = values;
  }
  const request = prepareEndpointRequest(parts, dynamicVars);

  // 路径参数缺值时不发出去：它按 OpenAPI 惯例是必填的，带着 `{id}` 发出去只会换一个
  // 含义不明的 404，而用户看不出是自己漏填了一个值。与单次发送同一道门。
  if (request.missingPathParams.length > 0) {
    return {
      ...failed(item, 'blocked', `路径参数没有取值：${request.missingPathParams.join('、')}`),
      missingPathParams: request.missingPathParams,
      missingVars: request.missingVars,
    };
  }

  const { response, script, outbound } = await sendPreparedRequest({
    request,
    auth: parts.effective.auth,
    settings: ctx.settings,
    signal,
    // 端点 id：后端据此装配脚本链（祖先集合的两段 + 端点自己的两段）。
    endpointId: item.endpointId,
    // 脚本看得见的环境。与解析用的是同一份变量池。
    env: parts.scriptEnv,
    // 端点自己的那两段脚本：已存的那份。
    preRequestScript: parts.effective.preRequestScript,
    testScript: parts.effective.testScript,
    // 该端点自己的 HTTPS 出站设置。`null` = 继承全局默认。
    tlsOverride: parts.effective.tls,
  });

  // 响应落进历史面板，事后可以逐条复核。这里存的是 `outbound`（发出去的那一份），
  // 不是预览那份：OAuth2 的 token 只在 `outbound` 里。
  const snapshot = toRequestSnapshot(outbound, response.requestHeaders);
  ctx.addResponseHistory(item.endpointId, response, snapshot);

  const outcome = classify(item, response, request.missingVars);
  // 脚本写回的环境变量由后端落库。本地的副本等整批跑完再统一同步——中途同步会
  // 让后面那几条看到被改过的变量，快照隔离就破了。
  if (script && script.envWrites.length > 0 && parts.scriptEnv?.id) {
    outcome.envWrites = script.envWrites;
    outcome.envId = parts.scriptEnv.id;
  }
  return outcome;
}

/** 一条失败的结论。 */
function failed(
  item: BatchItem,
  failure: 'http' | 'transport' | 'blocked' | 'aborted',
  error: string,
): BatchOutcome {
  return {
    endpointId: item.endpointId,
    name: item.name,
    method: item.method,
    success: false,
    status: 0,
    statusText: '',
    durationMs: 0,
    failure,
    error,
  };
}

/** 把一次发送的响应折成结论。 */
function classify(item: BatchItem, response: ApiResponse, missingVars: string[]): BatchOutcome {
  const base = {
    endpointId: item.endpointId,
    name: item.name,
    method: item.method,
    durationMs: response.durationMs,
    ...(missingVars.length > 0 ? { missingVars } : {}),
  };
  // 2xx 记成功。状态码 0 表示压根没拿到响应：超时、网络错误、OAuth2 取 token
  // 失败，或者用户中止了在途的那一条。
  if (response.status >= 200 && response.status < 300) {
    return {
      ...base,
      success: true,
      status: response.status,
      statusText: response.statusText,
      failure: null,
    };
  }
  if (response.status > 0) {
    return {
      ...base,
      success: false,
      status: response.status,
      statusText: response.statusText,
      failure: 'http',
      error: `HTTP ${response.status} ${response.statusText}`.trim(),
    };
  }
  const aborted = response.statusText === 'Aborted';
  return {
    ...base,
    success: false,
    status: 0,
    statusText: response.statusText,
    failure: aborted ? 'aborted' : 'transport',
    error: response.error ?? response.statusText,
  };
}
