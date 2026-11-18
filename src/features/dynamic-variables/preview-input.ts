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
 * 只看抽取规则与「有没有脚本」不够：换掉内联 URL、换一个数据源接口、换取数环境，或改掉
 * 脚本正文，拿到的都是另一份输入下的结果，却会被当成「当前草稿」的结果继续展示。
 */
export function previewFingerprint(input: PreviewInput): string {
  return JSON.stringify(previewPayload(input));
}
