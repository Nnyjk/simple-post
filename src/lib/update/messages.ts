/**
 * 更新检查结果的中文文案。`update_check` 失败时仍是 200，失败原因在 `error` 字段里，
 * 故这些文案是界面唯一能告诉用户「为什么没查到」的地方。
 *
 * 入参取 `string | null`：错误码是后端契约里的四个字面量，这里只做查表，不复制那份联合
 * 类型；没见过的码按「未知原因」呈现并把原码带上，界面不会因为一个新增的码而空着。
 */

const ERROR_TEXT: Record<string, string> = {
  unreachable: '连不上发布服务器（网络不可达，或被代理拦住了）',
  rate_limited: 'GitHub 接口限流，过几分钟再试',
  no_release: '仓库里还没有发布版本',
  bad_response: '发布接口返回了读不懂的内容',
};

/** 失败原因的中文说法；`null` 表示这次没有失败，返回 `null`。 */
export function describeCheckError(error: string | null | undefined): string | null {
  if (!error) return null;
  return ERROR_TEXT[error] ?? `检查失败（${error}）`;
}

/**
 * 一次检查完成后卡片上那句话。四类结果互斥：失败、有新版本、已是最新。
 * 「有新版本」把目标版本号带上，用户不必再打开对话框才知道是哪个版本。
 */
export function describeCheckOutcome(info: {
  available: boolean;
  latest: string | null;
  error: string | null;
}): string {
  const failure = describeCheckError(info.error);
  if (failure) return failure;
  if (info.available && info.latest) return `发现新版本 v${info.latest}`;
  return '已是最新版本';
}
