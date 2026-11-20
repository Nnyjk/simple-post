/**
 * `scripts/release-core.mjs` 的类型声明。
 *
 * 脚本本身是纯 JS（CLI 零依赖），故类型只在 import 侧声明：用例
 * `src/lib/__tests__/release-core.test.ts` 从这个 `.mjs` import 时要能过 `tsc -b`
 * （`allowJs` 关着，没有这份声明会报 TS7016）。
 */

/** 解析出来的版本号。 */
export interface ParsedVersion {
  major: number;
  minor: number;
  patch: number;
}

/** 解析出来的提交标题。 */
export interface ParsedCommit {
  type: string;
  scope: string;
  subject: string;
  breaking: boolean;
}

/** 说明里的一条改动。 */
export interface CommitEntry {
  text: string;
  scope: string;
  subject: string;
  breaking: boolean;
}

/** 说明里的一个小节；`title` 是中文小标题。 */
export interface CommitGroup {
  key: string;
  title: string;
  entries: CommitEntry[];
}

/** 组序，也是小标题的出现顺序。 */
export const GROUP_ORDER: string[];

/** 组键 → 中文小标题。 */
export const GROUP_TITLES: Record<string, string>;

/** 解析 `x.y.z`；解析不了返回 `null`。 */
export function parseVersion(text: string): ParsedVersion | null;

/** merge 提交（标题以 `Merge` 开头）。 */
export function isMergeCommit(title: string): boolean;

/** 发版自身的 `chore(release)` 提交。 */
export function isReleaseCommit(title: string): boolean;

/** 解析 Conventional Commits 标题；merge 或认不出的返回 `null`。 */
export function parseCommitTitle(title: string): ParsedCommit | null;

/** 按 type 分组，空组已经滤掉，顺序即 `GROUP_ORDER`。 */
export function groupCommits(titles: string[]): CommitGroup[];

/** 说明的正文小节（不含 `## Simple Post v…` 标题）。 */
export function renderReleaseSection(titles: string[]): string;

/** 一份独立的完整说明。 */
export function renderReleaseNotes(titles: string[], version: string): string;

/** 把改动小节嵌进 draft release 现有正文的 `### 这一版的改动` 位置。 */
export function mergeReleaseBody(body: string, section: string): string;

/** `patch` / `minor` / 显式 `x.y.z` → 下一个版本号；非法入参抛错。 */
export function resolveNextVersion(current: string, bump: string): string;
