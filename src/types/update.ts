// 「无头 web 端」更新检查三条 REST 路由的响应体镜像。
//
// 形状由 `crates/simple-post-server/src/update_api.rs` 与
// `crates/simple-post-core/src/update/check.rs` 决定，服务端字段全部 `camelCase`，
// 与线上 JSON 同名，故这里不做任何改名。
//
// 三条路都受 bearer 鉴权；`check` 的失败也是 HTTP 200（降级信息在 `error` 字段里），
// 只有 `apply` 会回 403 / 409。

/** `GET /api/update/check` 失败时的分类。`null` = 这次拿到了一份可用结果。 */
export type UpdateErrorCode = 'unreachable' | 'rate_limited' | 'no_release' | 'bad_response';

export interface UpdateCheck {
  /** 本进程的版本，即工作区版本。 */
  current: string;
  /** 最新版本，去掉 tag 的前导 `v`；没有可用 release 或出错时为 `null`。 */
  latest: string | null;
  /** `latest` 是否比 `current` 新。 */
  available: boolean;
  /** 这次判定发生的时刻（RFC3339 UTC）。命中缓存时是那次抓取的时刻。 */
  checkedAt: string;
  /** `true` = 这份结果来自服务端 6 小时缓存，没有出站。 */
  cached: boolean;
  /** 发布说明 markdown；可能为 `null`。 */
  notes: string | null;
  /** release 页地址；可能为 `null`。 */
  url: string | null;
  /** 失败分类；`null` = 成功（含「确实没有新版本」）。 */
  error: UpdateErrorCode | null;
}

/** 部署形态。`docker` 才有镜像引用与一行更新命令。 */
export type DeploymentShape = 'docker' | 'binary';

export interface UpdateDeployment {
  shape: DeploymentShape;
  /** 镜像引用，`shape === 'docker'` 时给出，否则 `null`。 */
  image: string | null;
  /** 给运维看的那行命令，`shape === 'docker'` 时给出，否则 `null`。 */
  command: string | null;
  /** 这台机器能不能执行更新：运维声明了更新命令 **且** 部署开了口令模式。 */
  applyEnabled: boolean;
}

/** `POST /api/update/apply` 的结果。不判断成败：退出码与输出就是命令本身的结果。 */
export interface UpdateApplyResult {
  /** 实际执行的命令串。 */
  command: string;
  /** 退出码；超时、启动失败或被信号终止时为 `-1`。 */
  exitCode: number;
  stdout: string;
  stderr: string;
}
