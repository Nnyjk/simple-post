/**
 * 「这个端点这一次要发什么」的推导 —— 应用内**唯一**的一份。
 *
 * 单次发送（`HttpTester`）与批量运行（`src/features/batch-run/`）都从这里取要素，故
 * 「批量运行的变量替换语义与单次发送完全一致」这条要求由结构保证，而不是靠两边
 * 各自小心。改动任何一条规则（例如 base URL 的解析次序、动态变量的压过规则）只改
 * 这一个文件，两侧同时生效。
 *
 * 推导出的要素与「组装请求」分成两段，原因是动态变量的值要在**替换之前**才拿到：
 * 拿到新值之后要重新组装一次，而重新组装需要的正是这里推导出的那几项。
 */
import { enabledVars, resolveEndpointBaseUrl, type ResolvedBaseUrl } from '@/lib/url';
import type { PreparedRequest } from '@/lib/http-build';
import type { EndpointDetail, Environment, EnvSnapshot, HttpMethod } from '@/types/domain';
import type { EndpointDraft } from '@/stores/store-types';
import { buildDynamicNames } from './dynamicVars';
import { buildEffectiveRequest, type EffectiveRequest } from './effectiveRequest';
import { buildRequestInput } from './requestInput';

/**
 * 一个端点一次发送的全部要素。
 *
 * `env` 是**解析用的**环境（`prepareRequest` 从它取变量池），`scriptEnv` 是**脚本
 * 看得见**的环境快照。两者看的是同一份变量池，但形态不同：前者是环境对象本身
 * （`buildPool` 从它的 `variables` 与 `disabledVariables` 合成），后者是已经滤掉
 * 停用变量的扁平字典。
 */
export interface EndpointRequestParts {
  /** 请求方法。草稿优先于已存的那份。 */
  method: HttpMethod;
  /** URL 的 path 部分，可带 `{{var}}`。草稿优先于已存的那份。 */
  urlPath: string;
  /** 已解析的 base URL；空串表示没有 base URL。 */
  baseUrl: string;
  /** 生效的 base URL 全貌：后端 api 地址、实际访问地址与来源。 */
  resolved: ResolvedBaseUrl;
  /**
   * 环境里「环境 × 模块」那一格的地址；没有那一格时是 `null`。
   *
   * 它与 `resolved.baseUrl` 分开：端点处在「自定义」档时 `resolved.baseUrl` 是端点
   * 自己的地址，而「跟随模块」那一行要显示的是这一格——用户据此判断「切回跟随模块
   * 会用哪个地址」。
   */
  moduleBaseUrl: string | null;
  /** 有效请求：已存的 detail 叠上草稿。 */
  effective: EffectiveRequest;
  /** 解析用的环境。 */
  env: Environment | undefined;
  /** 脚本看得见的环境快照。没有环境时是 `undefined`。 */
  scriptEnv: EnvSnapshot | undefined;
  /** 本次请求引用到的、且有定义且已启用的动态变量名。 */
  dynamicNames: string[];
}

/** 推导要素所需的输入。 */
export interface EndpointRequestPartsInput {
  /** 已存的那份请求配置。草稿接口也有（`useActiveEndpointResource` 合成的那份）。 */
  detail: EndpointDetail | undefined;
  /** 界面上此刻生效的未保存编辑。没有草稿时传 `undefined`。 */
  draft: EndpointDraft | undefined;
  /** 这个端点用的环境：工作区当前环境。 */
  endpointEnv: Environment | undefined;
  /** 端点所处的模块。base URL 的**值**按（环境 × 模块）那一格存放，故需要它。 */
  moduleId: string | null;
  /** 当前项目里已启用且仍存在的动态变量名。 */
  enabledDynamicNames: ReadonlySet<string>;
}

/**
 * 推导一个端点一次发送的全部要素。
 *
 * 这一步是纯函数：同样的输入给出同样的输出，故界面预览、cURL 预览与真实发送读到
 * 的必然是同一个请求。
 */
export function buildEndpointRequestParts(input: EndpointRequestPartsInput): EndpointRequestParts {
  const method = input.draft?.method ?? input.detail?.method ?? 'GET';
  const urlPath = input.draft?.url ?? input.detail?.url ?? '';
  const effective = buildEffectiveRequest(input.draft, input.detail);
  const resolved = resolveEndpointBaseUrl(
    input.detail ?? { baseUrlOverride: null },
    input.endpointEnv,
    input.moduleId,
  );
  const scriptEnv = input.endpointEnv
    ? { id: input.endpointEnv.id, variables: enabledVars(input.endpointEnv) }
    : undefined;
  // 动态变量的名单扫的是**未替换**的模板，故同名环境变量也会被扫出来——动态变量在
  // 替换时压过环境变量，故名单里只留有定义且已启用的那些。
  const dynamicNames = buildDynamicNames(
    resolved.baseUrl,
    urlPath,
    effective,
    resolved.proxy,
  ).filter((name) => input.enabledDynamicNames.has(name));
  return {
    method,
    urlPath,
    baseUrl: resolved.baseUrl,
    resolved,
    moduleBaseUrl: input.moduleId
      ? (input.endpointEnv?.moduleBaseUrls?.[input.moduleId]?.url ?? null)
      : null,
    effective,
    env: input.endpointEnv,
    scriptEnv,
    dynamicNames,
  };
}

/**
 * 用给定的动态变量值组装请求。
 *
 * `dynamicVars` 是**这一批**动态变量的当前值。传空对象即用不带动态变量的那份组装，
 * 与单次发送在「没有动态变量」时的结果一致。
 */
export function prepareEndpointRequest(
  parts: EndpointRequestParts,
  dynamicVars: Record<string, string>,
): PreparedRequest {
  return buildRequestInput(
    parts.method,
    parts.urlPath,
    parts.baseUrl,
    parts.resolved.proxy,
    parts.effective,
    parts.env,
    dynamicVars,
  );
}
