/**
 * scripts/release-core.mjs
 *
 * 发版脚本的**纯逻辑**：版本号运算、提交标题解析、发布说明分组渲染。
 *
 * 为什么把它单独拆出来：`scripts/` 不在 `npm test` 的采集范围里
 * （`scripts/run-tests.mjs` 只走 `src/**\/*.test.ts`），故这里一个副作用都不许有
 * —— 不读文件、不跑 git、不读环境变量。`scripts/release.mjs` 负责 I/O 与编排，
 * 用例 `src/lib/__tests__/release-core.test.ts` 直接 import 这个文件。
 *
 * 发布说明的唯一来源是提交标题（`git log --pretty=format:%s <上一个标签>..<新标签>`），
 * 不再手写。故「哪些标题算数、归到哪一组」就是这份文件的全部契约：
 *
 *   - merge 提交与 `chore(release)` 自身跳过（后者由发版脚本刚打出来，写进说明是废话）；
 *   - 按 Conventional Commits 的 type 分组，组序与中文小标题见 `GROUP_ORDER` /
 *     `GROUP_TITLES`；空组不输出；
 *   - 解析不出 type 的标题不丢，落进「其它」并原样保留 —— 一条改动在说明里消失，
 *     比它被归错组严重得多。
 *
 * 无依赖、无 I/O：Node ≥ 20 的 `.mjs`。
 */

/** 严格 `x.y.z`，不接受 `v` 前缀、预发布后缀或多出来的段。 */
const SEMVER = /^(\d+)\.(\d+)\.(\d+)$/;

/** `type(scope)!: subject`，Conventional Commits 的标题形状。 */
const CONVENTIONAL = /^([A-Za-z][\w-]*)(?:\(([^()]*)\))?(!)?:\s*(.+)$/;

/** 组序即说明里小标题的出现顺序：新功能 → 修复 → 重构 → 性能 → 测试与构建 → 其它。 */
export const GROUP_ORDER = ['feat', 'fix', 'refactor', 'perf', 'build-test', 'other'];

/** 组键 → 中文小标题。 */
export const GROUP_TITLES = {
  feat: '新功能',
  fix: '修复',
  refactor: '重构',
  perf: '性能',
  'build-test': '测试与构建',
  other: '其它',
};

/**
 * 生成小节自己的三级标题。
 *
 * 合并时用它们区分「占位节内部的组标题」与「真正的下一节」：上一轮合并过的正文里
 * `### 修复` 就在占位节里头，而 `### Install` / `## Simple Post v…` 才是边界。
 */
const SECTION_HEADINGS = new Set(Object.values(GROUP_TITLES));

/**
 * 解析 `x.y.z`。解析不了返回 `null`（不是抛错）：调用方对「当前版本读不出来」
 * 与「用户手输的版本号非法」要给出不同的提示。
 *
 * @param {string} text
 * @returns {{ major: number, minor: number, patch: number } | null}
 */
export function parseVersion(text) {
  const m = SEMVER.exec(String(text).trim());
  if (!m) return null;
  return { major: Number(m[1]), minor: Number(m[2]), patch: Number(m[3]) };
}

/**
 * 把提交标题归到组键上。
 *
 * `test` / `build` / `ci` 合成一组（它们都是「不改变用户可见行为」的那一类）；
 * 其余（`docs` / `style` / `chore` / `revert` / 认不出的 type）落进「其它」。
 *
 * @param {string} type
 * @returns {string}
 */
function groupOf(type) {
  if (type === 'feat' || type === 'fix' || type === 'refactor' || type === 'perf') return type;
  if (type === 'test' || type === 'build' || type === 'ci') return 'build-test';
  return 'other';
}

/**
 * merge 提交。rebase merge 之后 `main` 上通常没有它们，但直接推的标签范围里可能有
 * （例如从别的分支快进合并），故按标题判一次。
 *
 * @param {string} title
 * @returns {boolean}
 */
export function isMergeCommit(title) {
  return /^Merge\b/.test(String(title));
}

/**
 * 发版自身的提交（`chore(release): 0.1.40`）。写进说明只会得到「发版」一条。
 *
 * @param {string} title
 * @returns {boolean}
 */
export function isReleaseCommit(title) {
  const parsed = parseCommitTitle(title);
  return parsed !== null && parsed.type === 'chore' && parsed.scope === 'release';
}

/**
 * 解析一个提交标题。
 *
 * 解析不出 Conventional Commits 形状（包括 merge 提交）时返回 `null` ——
 * 调用方把它原样放进「其它」，而不是猜一个 type。
 *
 * @param {string} title
 * @returns {{ type: string, scope: string, subject: string, breaking: boolean } | null}
 */
export function parseCommitTitle(title) {
  const text = String(title).trim();
  if (text.length === 0 || isMergeCommit(text)) return null;
  const m = CONVENTIONAL.exec(text);
  if (!m) return null;
  return {
    type: m[1].toLowerCase(),
    scope: m[2] ? m[2].trim() : '',
    subject: m[4].trim(),
    breaking: m[3] === '!',
  };
}

/**
 * 一个条目在说明里的正文：`**scope**: subject`，不兼容变更带一句标注。
 *
 * @param {{ scope: string, subject: string, breaking: boolean }} parsed
 * @returns {string}
 */
function entryText(parsed) {
  const body = parsed.scope ? `**${parsed.scope}**: ${parsed.subject}` : parsed.subject;
  return parsed.breaking ? `${body}（不兼容变更）` : body;
}

/**
 * 把提交标题分组成说明的各个小节，空组已经不在了。
 *
 * `titles` 的顺序即说明里条目的顺序（调用方给 `git log` 的时间倒序，与 changelog
 * 习惯一致：最新的在上）。
 *
 * @param {string[]} titles
 * @returns {{ key: string, title: string, entries: { text: string, scope: string, subject: string, breaking: boolean }[] }[]}
 */
export function groupCommits(titles) {
  /** @type {Map<string, { text: string, scope: string, subject: string, breaking: boolean }[]>} */
  const buckets = new Map(GROUP_ORDER.map((key) => [key, []]));

  for (const raw of titles) {
    const title = String(raw).trim();
    if (title.length === 0 || isMergeCommit(title) || isReleaseCommit(title)) continue;

    const parsed = parseCommitTitle(title);
    const key = parsed ? groupOf(parsed.type) : 'other';
    // 解析不出 type 的标题原样保留：丢掉一条改动比归错组更糟。
    const entry = parsed
      ? { text: entryText(parsed), ...parsed }
      : { text: title, scope: '', subject: title, breaking: false };
    buckets.get(key)?.push(entry);
  }

  const groups = [];
  for (const key of GROUP_ORDER) {
    const entries = buckets.get(key) ?? [];
    if (entries.length === 0) continue;
    groups.push({ key, title: GROUP_TITLES[key], entries });
  }
  return groups;
}

/** 一条改动都没有时说明里留下的那句话（不是空字符串，否则正文看起来像构建失败）。 */
const EMPTY_NOTICE = '本次没有可归类的提交（`chore(release)` 与 merge 提交不计入）。';

/**
 * 说明的正文小节（不含文件标题）。`mergeReleaseBody` 用的就是它。
 *
 * @param {string[]} titles
 * @returns {string}
 */
export function renderReleaseSection(titles) {
  const groups = groupCommits(titles);
  if (groups.length === 0) return EMPTY_NOTICE;
  return groups
    .map((g) => [`### ${g.title}`, '', ...g.entries.map((e) => `- ${e.text}`)].join('\n'))
    .join('\n\n');
}

/**
 * 一份独立的完整说明 —— `gh release edit --notes-file` 在没有现成正文时用它。
 *
 * @param {string[]} titles
 * @param {string} version 不带 `v` 前缀的版本号
 * @returns {string}
 */
export function renderReleaseNotes(titles, version) {
  return `## Simple Post v${version}\n\n${renderReleaseSection(titles)}\n`;
}

/**
 * 把生成的改动小节嵌进 draft release 现有的正文里。
 *
 * 为什么不是整份替换：`release.yml` 的 `releaseBody` 里除了 `### 这一版的改动`
 * 占位（本节由这个脚本填）还有安装说明与容器镜像段落，那是用户真正要照着做的部分。
 * `gh release edit --notes-file` 是整份覆盖，故先把占位一节换掉再交出去。
 *
 * 占位节的边界是「`### 这一版的改动` 那一行」到「下一个 `## ` 标题之前」。
 * 找不到那个标题（模板改过）时不猜，把改动小节放在正文最前面，保证说明在手。
 *
 * @param {string} body 现有正文
 * @param {string} section `renderReleaseSection` 的输出
 * @returns {string}
 */
export function mergeReleaseBody(body, section) {
  const text = String(body ?? '');
  const lines = text.split('\n');
  const start = lines.findIndex((line) => /^###\s+这一版的改动\s*$/.test(line));

  if (start < 0) return `${section}\n\n${text}`.trimEnd() + '\n';

  // 下一个真标题：`### 修复` 这类**我们自己生成的组标题**不算边界（上一轮合并后它们就在
  // 占位节内部），任何别的 1–3 级标题才是。只看 `^## ` 会漏掉「标题在最前、安装说明在
  // 后面」的正文 —— 那里根本没有第二个 `## `，于是 `### Install` 到文件尾被整段截掉。
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i += 1) {
    const heading = /^#{1,3}\s+(.+?)\s*$/.exec(lines[i]);
    if (heading === null) continue;
    if (lines[i].startsWith('### ') && SECTION_HEADINGS.has(heading[1])) continue;
    end = i;
    break;
  }

  const head = lines.slice(0, start).join('\n').trimEnd();
  const tail = lines.slice(end).join('\n').trimEnd();
  const merged = [head, `### 这一版的改动`, '', section, tail]
    .filter((p) => p.length > 0)
    .join('\n\n');
  return `${merged}\n`;
}

/**
 * 算出下一个版本号。
 *
 * 本项目**长期停在 `0.1.x`，只累加最后一位，且没有上限**：`0.1.99` → `0.1.100` →
 * `0.1.101` → … 是正常的一步，不存在「攒够再升 0.2.0」这回事，故没有 `minor` / `major`
 * 这两档（曾经有 `minor`，会被当成非法入参报错并说明原因）。
 *
 * - 省略或 `patch` → 第三段 +1；
 * - `x.y.z` → 原样采用，但必须严格大于当前版本。这是**逃生口**，用来修「标签打错成 0.1.42
 *   而实际是 0.1.37」这类事故，不是日常发版路径。
 *
 * 末段比较按**段数值**而非字符串：`0.1.100` 比 `0.1.99` 新。任何一处退回字符串比较，
 * 跨过三位数的那一步就会坏。
 *
 * @param {string} current 当前版本号，例如 `0.1.39`
 * @param {string} bump `patch` / `x.y.z`
 * @returns {string}
 */
export function resolveNextVersion(current, bump) {
  const now = parseVersion(current);
  if (!now) throw new Error(`当前版本号不是 x.y.z：${current}`);
  // 省略参数即 `patch`（默认的快步迭代）；显式空串仍然算非法入参。
  const arg = bump === undefined || bump === null ? 'patch' : String(bump).trim();

  if (arg === 'patch') return `${now.major}.${now.minor}.${now.patch + 1}`;
  if (arg === 'minor' || arg === 'major') {
    throw new Error(
      `本项目的版本只累加最后一位（长期 0.1.x，且没有上限）：用 patch / 省略参数，` +
        `不要用 ${arg}`,
    );
  }

  const explicit = parseVersion(arg);
  if (!explicit) {
    throw new Error(`版本参数只能是 patch 或显式 x.y.z，收到：${arg === '' ? '（空）' : arg}`);
  }
  const compare = (a, b) => a.major - b.major || a.minor - b.minor || a.patch - b.patch;
  if (compare(explicit, now) <= 0) {
    throw new Error(`目标版本 ${arg} 必须大于当前版本 ${current}`);
  }
  return `${explicit.major}.${explicit.minor}.${explicit.patch}`;
}
