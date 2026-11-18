/**
 * 预览结果的「身份」：哪几格输入决定了手上的这份响应。
 *
 * 抽成不依赖 React 的模块，是为了能直接单测：界面里那些判断（结果还算不算当前草稿、
 * 连点两次哪一次上屏）全落在这个指纹上。
 */
export interface PreviewInput {
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

/**
 * 发起时那一份输入的指纹。
 *
 * 只看抽取规则与「有没有脚本」不够：换掉内联 URL、换一个数据源接口、换取数环境，或改掉
 * 脚本正文，拿到的都是另一份输入下的结果，却会被当成「当前草稿」的结果继续展示。
 */
export function previewFingerprint(input: PreviewInput): string {
  return JSON.stringify([
    input.sourceEndpointId,
    input.sourceInlineJson,
    input.environmentId,
    input.extractPath,
    input.transformScript,
    input.activeEnvironmentId,
  ]);
}
