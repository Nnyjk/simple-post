/**
 * 路径参数缺值的提示文案。
 *
 * 缺值可能来自两处：端点自己的路径（「路径参数」那一页有对应的行可填），或前置 URL
 * （模块地址）。后者在那一页没有行可填，提示必须说清去哪儿改 —— 否则用户被拦在一扇
 * 打不开的门外面。
 */
export function buildPathParamError(input: {
  /** 上一次发送是不是被路径参数缺值拦下了。 */
  sendBlocked: boolean;
  /** 当前请求里还缺值的路径参数名。 */
  missingPathParams: string[];
  /** 端点自己的路径里的占位符名。 */
  ownPathParams: string[];
  /** 前置 URL 里的占位符名。 */
  basePathParams: string[];
}): string | null {
  const { sendBlocked, missingPathParams, ownPathParams, basePathParams } = input;
  if (!sendBlocked || missingPathParams.length === 0) return null;
  const missingInBase = missingPathParams.filter(
    (n) => !ownPathParams.includes(n) && basePathParams.includes(n),
  );
  const missingInPath = missingPathParams.filter((n) => !missingInBase.includes(n));
  return missingInBase.length > 0
    ? `前置 URL 里的 ${missingInBase.join('、')} 不是端点自己的路径参数，请把前置 URL 改成 {{变量}} 或直接写明地址`
    : `路径参数 ${missingInPath.join('、')} 没有填值，填好才能发送`;
}
