/**
 * 预览结果的「身份」：哪几格输入决定了手上的这份响应。
 *
 * 抽成不依赖 React 的模块，是为了能直接单测：界面里那些判断（结果还算不算当前草稿、
 * 连点两次哪一次上屏）全落在这个指纹上。
 */
export interface PreviewInput {
  /** 数据源用哪一种形态：决定另两格是否参与取数。 */
  usesEndpoint: boolean;
  /** 引用的数据源接口 id；内联请求时为空串。 */
  sourceEndpointId: string;
  /** 内联请求正文；引用接口时为空串。 */
  sourceInlineJson: string;
  /** 定义里点名的取数环境；空串 = 继承。 */
  environmentId: string;
  /** 抽取规则原文。 */
  extractPath: string;
  /** 处理脚本原文（改一个字符就是另一份输入）。 */
  transformScript: string;
  /** 发起时调用方选中的环境：继承的定义按它取数。 */
  activeEnvironmentId: string | null;
}

/** 发给 `dynamic_variables_preview` 的那份参数：形态把不相干的两格归零。 */
export type PreviewPayload = {
  sourceEndpointId: string | null;
  sourceInlineJson: string | null;
  sourceEnvironmentId: string | null;
  extractPath: string;
  transformScript: string;
  environmentId: string | null;
};

/**
 * 把草稿归一成预览请求——也就是「这次取数究竟按什么取」。
 *
 * 指纹直接序列化它：两种情况填满两格再切形态时，指纹也会变，因为请求本身变了。
 */
export function previewPayload(input: PreviewInput): PreviewPayload {
  return {
    sourceEndpointId: input.usesEndpoint ? input.sourceEndpointId || null : null,
    sourceInlineJson: input.usesEndpoint ? null : input.sourceInlineJson || null,
    sourceEnvironmentId: input.usesEndpoint ? input.environmentId || null : null,
    extractPath: input.extractPath,
    transformScript: input.transformScript,
    environmentId: input.activeEnvironmentId,
  };
}

/**
 * 发起时那一份输入的指纹。
 *
 * 只看抽取规则与「有没有脚本」不够：换掉内联 URL、换一个数据源接口，或改掉脚本正文，拿到的
 * 都是另一份输入下的结果，界面却会把它当成「当前草稿」的结果继续展示。
 *
 * **环境不进指纹**。一份响应到底在哪个环境下取到，只有后端说得准：定义点名了就用点名的那个，
 * 否则继承调用方环境，而调用方环境不属于数据源接口那个项目时还会回落到那儿的默认环境。前端
 * 复刻这套判定只会错——按调用方环境算指纹，本项目里切一下环境就会把同一份结果标成「对应旧
 * 输入」，还诱导用户再点一次「预览取数」（非幂等接口就是对端生效两次）。
 *
 * 环境那一路由界面直接说明：结果旁边一直写着「环境：X」，与此刻会生效的环境不一致时再补一行
 * 「这份结果取自 X」（见 `previewExpectedEnvironment`）。
 */
export function previewFingerprint(input: PreviewInput): string {
  const payload = previewPayload(input);
  return JSON.stringify({
    sourceEndpointId: payload.sourceEndpointId,
    sourceInlineJson: payload.sourceInlineJson,
    extractPath: payload.extractPath,
    transformScript: payload.transformScript,
  });
}

/**
 * 此刻再点一次「预览取数」会用的环境（`null` = 用不到环境，或还没有可用环境）。
 *
 * 定义点了名就用点名的那个，否则用调用方当前环境的那个；内联请求自带完整地址，用不到环境。
 * 这只是**界面能算出来的那一档**：后端还会在环境不属于数据源接口所在项目时回落到那儿的默认
 * 环境，那一步前端不复刻。故它只用来判断「结果旁边的环境是不是此刻会生效的那个」，不参与
 * 指纹。
 */
export function previewExpectedEnvironment(input: PreviewInput): string | null {
  if (!input.usesEndpoint) return null;
  return input.environmentId || input.activeEnvironmentId;
}

/**
 * 失败归谁：指纹 **加上此刻会生效的环境**。
 *
 * 比成功结果多认环境这一档，是因为两者的信息量不同：一份成功的响应自带「环境：X」，界面能
 * 直接说出来，故指纹只判「来源/规则/脚本还是不是这一份」；而失败只有一句原因，跟哪个环境有
 * 关系看不出来——同一条网络故障换个环境就是另一回事。少了这一档，A 的失败会挂在 B 名下。
 */
export function previewFailureKey(fingerprint: string, expectedEnvironment: string | null): string {
  return `${fingerprint}\u0000${expectedEnvironment ?? ''}`;
}

/** 预览失败也只属于交出它的那份输入。 */
export interface PreviewFailure {
  /** `previewFailureKey`：发起时那份输入的指纹 + 会生效的环境。 */
  key: string;
  /** 后端或传输层给的原因。 */
  message: string;
}

/**
 * 这条失败该不该显示在当前草稿下面。
 *
 * 键对不上就是「上一份输入（或另一个环境）的失败」：把它摆在当前草稿名下，用户会以为刚改的
 * 这份也失败了。
 */
export function previewFailureMessage(failure: PreviewFailure | null, key: string): string | null {
  if (failure === null || failure.key !== key) return null;
  return failure.message;
}
