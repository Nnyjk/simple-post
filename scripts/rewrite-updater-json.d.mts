/**
 * `scripts/rewrite-updater-json.mjs` 的类型声明。
 *
 * 脚本本身是纯 JS（CLI 零依赖），故类型只在 import 侧声明：单测从
 * `src/__tests__/rewrite-updater-json.test.ts` import 它时要能过 `tsc -b`。
 */

/** `gh api repos/<repo>/releases/tags/<tag>` 响应里 `assets[]` 的一项。 */
export interface ReleaseAsset {
  id: number;
  name: string;
  browser_download_url?: string;
  [key: string]: unknown;
}

/** `gh api repos/<repo>/releases/tags/<tag>` 响应里本脚本读到的两个字段。 */
export interface Release {
  tag_name?: string;
  assets?: ReleaseAsset[];
  [key: string]: unknown;
}

/** latest.json 里一个平台的条目；`url` 会被改写，其余字段原样带过去。 */
export interface PlatformEntry {
  signature?: string;
  url?: string;
  [key: string]: unknown;
}

/** latest.json 的形状。 */
export interface UpdaterJson {
  version?: string;
  notes?: string;
  pub_date?: string;
  platforms: Record<string, PlatformEntry>;
  [key: string]: unknown;
}

/**
 * 把每个平台的 url 从 API 资产端点改写成 release 下载地址，返回新对象。
 *
 * 认不出的 url 抛错，错误信息里带平台键名与那个 url。
 */
export function rewriteUpdaterJson(latest: UpdaterJson, release: Release): UpdaterJson;
