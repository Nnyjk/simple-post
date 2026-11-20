#!/usr/bin/env node
/**
 * scripts/release.mjs
 *
 * 一条命令完成一次发版：
 *
 *   升版本号（四处同步 + 两个锁文件）→ 分支 + PR + rebase merge → 注解标签
 *   →（`--publish`）等 release.yml **全部作业**成功 → 填发布说明 → 取消草案
 *
 * Usage:
 *   npm run release                      # 默认 patch：只累加最后一位（0.1.39 → 0.1.40）
 *   npm run release -- patch --dry-run   # 只打印计划与发布说明，一个字节都不写
 *   npm run release -- patch --publish   # 等流水线成功并取消草案（真正发出去）
 *   npm run release -- 0.1.42            # 逃生口：显式版本，修打错的标签
 *
 * 版本规则：长期停在 `0.1.x`，只累加最后一位且没有上限（`0.1.99` → `0.1.100` 是正常
 * 一步）；没有 `minor` / `major` 这两档，`scripts/release-core.mjs` 会把它们当场拒掉。
 *
 * 为什么 `--publish` 要等 `fix-updater-json`：`release.yml` 的三个矩阵作业用
 * `releaseDraft: true` 出包，tauri-action 会把 `latest.json` 的 `platforms.*.url`
 * 写成 `https://api.github.com/repos/<owner>/<repo>/releases/assets/<id>`。那个端点是
 * API 端点、要求 `User-Agent`，而 `tauri-plugin-updater` 用的 reqwest 默认不发，
 * 于是应用内更新拿到 `403`（0.1.35 → 0.1.36 就是这么坏的）。后置作业
 * `fix-updater-json` 把它改写成公开下载地址后才可公布；在那之前公布 = 给每个用户
 * 一个装不上的更新。
 *
 * 为什么走 PR：`main` 受保护（`enforce_admins` 打开），直接推会被拒。PR 用
 * **rebase merge** 落地 —— rebase 保留提交正文，squash 会用 PR 标题替换它。
 *
 * 为什么发布说明由提交标题生成：`v0.1.x` 是快步迭代，手写说明的边际成本远高于
 * 它的信息量；`git log --pretty=format:%s <上一个标签>..<新标签>`（与
 * `git log --oneline` 的标题字段完全相同）就是这一版做了什么。分组规则在
 * `scripts/release-core.mjs`，用例在 `src/lib/__tests__/release-core.test.ts`。
 *
 * 这个文件只做 I/O 与编排；任何能写成纯函数的东西都在 `release-core.mjs` 里。
 */
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  mergeReleaseBody,
  renderReleaseNotes,
  renderReleaseSection,
  resolveNextVersion,
} from './release-core.mjs';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');

/** `--publish` 时等流水线 / 草案 / 发布稿各自的上限；都可用环境变量缩短（便于演练）。 */
const RUN_APPEAR_TIMEOUT_MS = envMs('SIMPLE_POST_RELEASE_RUN_APPEAR_MS', 3 * 60_000);
const RUN_TIMEOUT_MS = envMs('SIMPLE_POST_RELEASE_RUN_MS', 120 * 60_000);
const DRAFT_TIMEOUT_MS = envMs('SIMPLE_POST_RELEASE_DRAFT_MS', 3 * 60_000);
const POLL_INTERVAL_MS = envMs('SIMPLE_POST_RELEASE_POLL_MS', 20_000);

/** 一次发版会动的全部文件 —— 出现第七个文件就说明有东西跑偏了。 */
const EXPECTED_FILES = [
  'package.json',
  'Cargo.toml',
  'src-tauri/tauri.conf.json',
  'README.md',
  'Cargo.lock',
  'package-lock.json',
];

/**
 * 版本号的四处来源。模式里带版本号本身，是为了让「找到 0 处」变成硬失败：
 * 换了写法却静默跳过，发出去的就是一个版本号自相矛盾的包。
 */
const VERSION_SOURCES = [
  {
    file: 'package.json',
    describe: '顶层 version 字段',
    pattern: (v) => new RegExp(`"version": "${escapeRe(v)}"`, 'g'),
    replacement: (v) => `"version": "${v}"`,
  },
  {
    file: 'Cargo.toml',
    describe: '[workspace.package] version',
    pattern: (v) => new RegExp(`^version = "${escapeRe(v)}"$`, 'gm'),
    replacement: (v) => `version = "${v}"`,
  },
  {
    file: 'src-tauri/tauri.conf.json',
    describe: 'version 字段',
    pattern: (v) => new RegExp(`"version": "${escapeRe(v)}"`, 'g'),
    replacement: (v) => `"version": "${v}"`,
  },
  {
    file: 'README.md',
    describe: '版本徽章 `version-v<x.y.z>`',
    pattern: (v) => new RegExp(`version-v${escapeRe(v)}-`, 'g'),
    replacement: (v) => `version-v${v}-`,
  },
];

const USAGE = `用法：npm run release -- [patch|<x.y.z>] [--dry-run] [--publish]

  默认 patch：本项目的版本长期停在 0.1.x，只累加最后一位，无上限
  （0.1.99 → 0.1.100 是正常一步，不存在「攒够升 0.2.0」）。

  显式 x.y.z 是逃生口（修打错的标签），必须大于当前版本，不是日常路径。

  --dry-run   只打印计划与生成的发布说明，不建分支、不提交、不打标签、不调 gh 写命令
  --publish   标签推送后等 release.yml 全部作业成功，填好说明并取消草案`;

// ── 日志 ───────────────────────────────────────────────────────────────────

const log = (msg) => console.log(`[release] ${msg}`);
const warn = (msg) => console.warn(`[release] 警告：${msg}`);

/** 失败即退出非零码：发版脚本没有「继续试试」这种中间态。 */
function fail(message, hint) {
  console.error(`[release] 错误：${message}`);
  if (hint) console.error(`[release] 处理：${hint}`);
  process.exit(1);
}

function sleep(ms) {
  return new Promise((done) => setTimeout(done, ms));
}

function envMs(name, fallback) {
  const raw = process.env[name];
  if (!raw) return fallback;
  const value = Number(raw);
  return Number.isFinite(value) && value > 0 ? value : fallback;
}

const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** 打印用：把一条命令还原成可复制的形式（只用于展示，不用于执行）。 */
const formatCommand = (cmd, args) => [cmd, ...args].join(' ');

// ── 执行 ───────────────────────────────────────────────────────────────────

/**
 * 跑一个外部命令。
 *
 * `git` / `gh` / `cargo` 在三个平台上都是真正的可执行文件，故用参数数组、
 * 不经 shell —— 提交标题、PR 正文这类含括号冒号中文的字符串因此不需要任何转义。
 * stderr 一律透出来（`capture` 时手工转到我们的 stderr），否则 `gh` 的权限报错
 * 会被吞掉，只剩一句「命令失败」。
 *
 * @returns {{ ok: boolean, status: number, stdout: string, stderr: string }}
 */
function exec(cmd, args, { capture = false, timeoutMs = 0, quiet = false } = {}) {
  const res = spawnSync(cmd, args, {
    cwd: ROOT,
    stdio: capture ? ['ignore', 'pipe', 'pipe'] : ['inherit', 'inherit', 'pipe'],
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
    timeout: timeoutMs > 0 ? timeoutMs : undefined,
  });

  const stdout = res.stdout ?? '';
  const stderr = res.stderr ?? '';
  // `stdio: 'inherit'` 时 stderr 仍然被 pipe 住，以便这里统一决定要不要显示。
  // `quiet` 只用于轮询探针：它按设计会反复拿到同一条「还没有」的错误。
  if (stderr.length > 0 && !quiet) process.stderr.write(stderr);
  if (res.error) {
    fail(`无法执行 ${cmd}：${res.error.message}`, `${cmd} 是否在 PATH 上？`);
  }
  return { ok: res.status === 0, status: res.status ?? 1, stdout, stderr };
}

function must(cmd, args, opts) {
  const res = exec(cmd, args, opts);
  if (!res.ok) {
    fail(`${formatCommand(cmd, args)} 失败（退出码 ${res.status}）`);
  }
  return res;
}

/**
 * `npm` 在 Windows 上只有 `.cmd` 外壳，必须经 shell 才能启动。
 *
 * 参数是脚本里写死的字面量（没有用户输入、没有空格），故拼成一条命令行是安全的
 * —— 与 `scripts/run-tests.mjs` 同一处理方式，也避开了 Node 对
 * 「`shell: true` + 参数数组」的弃用告警。
 */
function runNpm(args) {
  const line = ['npm', ...args].join(' ');
  const res = spawnSync(line, { cwd: ROOT, stdio: 'inherit', shell: true });
  if (res.error) fail(`无法执行 ${line}：${res.error.message}`);
  if (res.status !== 0) fail(`${line} 失败（退出码 ${res.status}）`);
}

/** 轮询到 `fn` 返回真值；超时按「等不到就是失败」处理。 */
async function poll(what, fn, { timeoutMs, hint }) {
  const started = Date.now();
  for (;;) {
    const value = fn();
    if (value) return value;
    const spent = Date.now() - started;
    if (spent > timeoutMs) {
      fail(`${what}：等了 ${Math.round(spent / 60_000)} 分钟仍没有结果`, hint);
    }
    await sleep(POLL_INTERVAL_MS);
  }
}

// ── 参数与只读事实 ─────────────────────────────────────────────────────────

/**
 * npm 会把脚本名之后的 `--dry-run` / `--publish` 当成自己的配置吃掉（`npm_config_dry_run=true`），
 * 脚本的 `process.argv` 里一个都看不到 —— 于是 `npm run release --dry-run` 会**真的发一次版**。
 * 只有 `npm run release -- --dry-run` 才透传。为了不让人踩这个坑，把 npm 转发到环境里的同名
 * 配置折回参数列表，并说明正确写法。
 */
function argvWithNpmConfig(argv) {
  const extra = [];
  for (const [env, flag] of [
    ['npm_config_dry_run', '--dry-run'],
    ['npm_config_publish', '--publish'],
    ['npm_config_help', '--help'],
  ]) {
    const value = process.env[env];
    if (value === undefined || value === 'false') continue;
    if (argv.includes(flag)) continue;
    extra.push(flag);
    warn(
      `npm 吃掉了 ${flag}（它把它当自己的配置）；已按环境变量补回。正确写法是 npm run release -- ${flag}`,
    );
  }
  return [...argv, ...extra];
}

function parseArgs(argv) {
  const out = { bump: 'patch', dryRun: false, publish: false, help: false };
  const positional = [];
  for (const arg of argv) {
    if (arg === '--dry-run') out.dryRun = true;
    else if (arg === '--publish') out.publish = true;
    else if (arg === '-h' || arg === '--help') out.help = true;
    else if (arg.startsWith('-')) throw new Error(`未知参数：${arg}`);
    else positional.push(arg);
  }
  if (positional.length > 1) {
    throw new Error(`只接受一个版本参数，收到：${positional.join(' ')}`);
  }
  if (positional.length === 1) out.bump = positional[0];
  return out;
}

/** 当前版本号以 `package.json` 为准（它是 npm / tauri / cargo 三处里最靠前的那一份）。 */
function readCurrentVersion() {
  let pkg;
  try {
    pkg = JSON.parse(readFileSync(join(ROOT, 'package.json'), 'utf8'));
  } catch (err) {
    fail(`读不出 package.json：${err.message}`);
  }
  if (typeof pkg.version !== 'string') fail('package.json 里没有 version 字段');
  return pkg.version;
}

/** `git log --pretty=format:%s <range>` —— 与 `git log --oneline` 的标题字段一致。 */
function readTitles(range) {
  const { stdout } = must('git', ['log', '--pretty=format:%s', range], { capture: true });
  return stdout
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

// ── 版本号：核对、改写、锁文件 ─────────────────────────────────────────────

/**
 * 只读核对：每一处都能恰好找到一次当前版本号。
 *
 * dry-run 也走这一步 —— 打印一份「不会成功」的计划没有意义。
 */
function inspectVersionSources(from) {
  const found = [];
  for (const source of VERSION_SOURCES) {
    const abs = join(ROOT, source.file);
    let src;
    try {
      src = readFileSync(abs, 'utf8');
    } catch (err) {
      fail(`读不出 ${source.file}：${err.message}`, '仓库不完整？先 git status 看看');
    }
    const hits = src.match(source.pattern(from)) ?? [];
    if (hits.length !== 1) {
      fail(
        `${source.file} 里找到 ${hits.length} 处「${source.describe}」（版本 ${from}），预期恰好 1 处`,
        '版本号的写法变了？先修 scripts/release.mjs 里的模式，别手改后绕过去',
      );
    }
    found.push(source);
  }
  return found;
}

/** 数一处来源的匹配次数。`pattern()` 每次新建带 `g` 的正则，故不踩 `lastIndex` 的状态。 */
const countMatches = (text, source, version) => (text.match(source.pattern(version)) ?? []).length;

function applyVersionSources(from, to) {
  for (const source of inspectVersionSources(from)) {
    const abs = join(ROOT, source.file);
    const src = readFileSync(abs, 'utf8');
    const next = src.replace(source.pattern(from), source.replacement(to));
    if (next === src) fail(`${source.file} 没有发生替换（${from} → ${to}）`);
    writeFileSync(abs, next);

    // 写完再核一遍：旧版本号一处不剩、新版本号恰好一处 —— 与 `inspectVersionSources` 同一口径。
    //
    // 不能拿 `includes` 判：`0.1.390` 里含 `0.1.39`，`written.includes(from)` 照样为真，核对
    // 会放行一个还留着旧引用的文件，发出去的就是个版本号自相矛盾的包。
    const written = readFileSync(abs, 'utf8');
    const stale = countMatches(written, source, from);
    const fresh = countMatches(written, source, to);
    if (stale !== 0 || fresh !== 1) {
      fail(
        `${source.file} 写完核对失败（${source.describe}）：旧版本还剩 ${stale} 处、新版本 ${fresh} 处`,
        `预期 0 与 1 —— 检查这个文件里的 ${from} 与 ${to}`,
      );
    }
  }
}

/**
 * 让两个锁文件跟上新版本号。
 *
 * `cargo metadata` 会按 `Cargo.toml` 重算并写回 `Cargo.lock` 里工作区那几个 crate 的版本行；
 * `npm install --package-lock-only` 同理由 npm 重写 `package-lock.json`。手改锁文件迟早会漏
 * 一个 crate。
 *
 * **不能用 `--no-deps`**：那一档只读元数据、根本不落盘，`Cargo.lock` 一个字节都不动，
 * 于是 `verifyDiff` 每次都判定「Cargo.lock 没有变化」而中止发版。加 `--offline` 之外不加
 * 别的：依赖若被顺手重解，`verifyDiff` 的版本行白名单会拦下来（这正是它存在的理由）。
 */
function refreshLockfiles() {
  must('cargo', ['metadata', '--format-version', '1'], { capture: true });
  runNpm(['install', '--package-lock-only']);
}

/**
 * 提交前核 diff：只许出现预期的六个文件，且两个锁文件的改动只许是版本行。
 */
function verifyDiff(current, next) {
  const { stdout } = must('git', ['diff', '--name-only', 'HEAD'], { capture: true });
  const changed = stdout
    .split('\n')
    .map((line) => line.trim().replaceAll('\\', '/'))
    .filter((line) => line.length > 0);

  const unexpected = changed.filter((file) => !EXPECTED_FILES.includes(file));
  if (unexpected.length > 0) {
    fail(
      `出现了预期外的改动：\n  ${unexpected.join('\n  ')}`,
      '这些改动不该进发版提交；清理掉（git checkout -- <文件>）再重跑',
    );
  }
  const missing = EXPECTED_FILES.filter((file) => !changed.includes(file));
  if (missing.length > 0) {
    fail(
      `这些文件没有变化：\n  ${missing.join('\n  ')}`,
      '锁文件没更新？cargo metadata / npm install --package-lock-only 没跑成？',
    );
  }

  // 锁文件的 diff 只该是 `version = "0.1.39"` 这类行；出现别的说明依赖被顺手解了。
  for (const lock of ['Cargo.lock', 'package-lock.json']) {
    const diff = must('git', ['diff', '-U0', '--', lock], { capture: true }).stdout;
    const stray = diff
      .split('\n')
      .filter((line) => /^[+-][^+-]/.test(line))
      .filter((line) => !line.includes(current) && !line.includes(next));
    if (stray.length > 0) {
      fail(
        `${lock} 里有与版本号无关的改动：\n  ${stray.slice(0, 20).join('\n  ')}`,
        '锁文件被重新解析了；把发行版依赖的顺手升级拆成另一个 PR',
      );
    }
  }

  log('改动清单：');
  process.stdout.write(must('git', ['diff', '--stat', 'HEAD'], { capture: true }).stdout);
}

// ── git / gh 编排 ──────────────────────────────────────────────────────────

function assertPreconditions() {
  const dirty = exec('git', ['status', '--porcelain'], { capture: true }).stdout.trim();
  if (dirty.length > 0) {
    fail(`工作区有未提交的改动：\n${dirty}`, '先提交或 stash —— 发版提交必须只含那六个文件');
  }

  const branch = must('git', ['rev-parse', '--abbrev-ref', 'HEAD'], {
    capture: true,
  }).stdout.trim();
  if (branch !== 'main') {
    fail(`当前分支是 ${branch}，发版必须从 main 开始`, 'git checkout main && git pull --ff-only');
  }

  must('git', ['fetch', '--tags', 'origin']);
  const local = must('git', ['rev-parse', 'main'], { capture: true }).stdout.trim();
  const remote = must('git', ['rev-parse', 'origin/main'], { capture: true }).stdout.trim();
  if (local !== remote) {
    fail('本地 main 与 origin/main 不一致', 'git pull --ff-only origin main 之后再发版');
  }
}

/**
 * 分支 / 标签撞名时**不自动清理**：一个已存在的发版分支或标签可能正是上一次发版
 * 走到一半的证据，脚本替人删掉它，等于把「那次到底发没发出去」变成猜谜。
 */
function assertFreeName(branch, tag) {
  const hint = '先确认上一次发版走到哪一步（gh pr list / gh release view），再人工决定怎么收拾';
  if (exec('git', ['rev-parse', '--verify', '--quiet', `refs/heads/${branch}`]).ok) {
    fail(`本地分支 ${branch} 已存在`, `${hint}；要重来先 git branch -D ${branch}`);
  }
  if (exec('git', ['ls-remote', '--exit-code', '--heads', 'origin', branch]).ok) {
    fail(`origin 上已有分支 ${branch}`, hint);
  }
  if (exec('git', ['rev-parse', '--verify', '--quiet', `refs/tags/${tag}`]).ok) {
    fail(`本地标签 ${tag} 已存在`, hint);
  }
  if (exec('git', ['ls-remote', '--exit-code', '--tags', 'origin', `refs/tags/${tag}`]).ok) {
    fail(`origin 上已有标签 ${tag}`, `${hint}；标签已推送就不能重打`);
  }
}

/** PR 正文里的预告：本分支相对上一个标签的提交标题。 */
function prBody(titles, current, next) {
  return [
    `版本号提升到 \`${next}\`（上一版 \`${current}\`）。`,
    '',
    '四个版本号来源与两个锁文件由 `scripts/release.mjs` 一次改完；',
    '发布说明由提交标题生成，故这里就是这一版的改动：',
    '',
    renderReleaseSection(titles),
    '',
  ].join('\n');
}

function createPr(branch, next, titles, current) {
  must('gh', [
    'pr',
    'create',
    '--base',
    'main',
    '--head',
    branch,
    '--title',
    `chore(release): ${next}`,
    '--body',
    prBody(titles, current, next),
  ]);

  const { stdout } = must('gh', ['pr', 'view', branch, '--json', 'number,url'], { capture: true });
  let info;
  try {
    info = JSON.parse(stdout);
  } catch {
    fail(`读不出刚建的 PR：${stdout.trim()}`, 'gh pr view <branch> 手工确认一下');
  }
  return info;
}

/**
 * 等 PR 上的检查结束。`--fail-fast` 让第一个红勾立刻回报，不必等满整条矩阵。
 *
 * 两个坑：`gh pr checks` 在 check-suite 还没挂到 head 提交上时以
 * `no checks reported on the '<branch>' branch` 退出非零，而 `gh pr create` 刚返回的那几秒
 * 正是这个窗口 —— 直接 `--watch` 会把「还没开始」当成「失败了」。故先静默轮询到真有检查，
 * 再进 watch；watch 期间 pending 归零才退出，故退出码非零就是真有红勾。
 */
async function awaitChecks(prNumber) {
  await poll(
    `PR #${prNumber} 的检查`,
    () => {
      // 退出码不能用：检查还在跑时 `gh pr checks` 以 8 退出（`Checks pending`），
      // 「还没挂上」是空 stdout。判据只有一个 —— 拿得到至少一条检查。
      const { stdout } = exec('gh', ['pr', 'checks', String(prNumber), '--json', 'name'], {
        capture: true,
        quiet: true,
      });
      try {
        const list = JSON.parse(stdout);
        return Array.isArray(list) && list.length > 0;
      } catch {
        return false;
      }
    },
    {
      timeoutMs: RUN_APPEAR_TIMEOUT_MS,
      hint: 'PR 建好了但一个检查都没挂上：CI 的 on: pull_request 触发了吗？gh pr view 看一眼',
    },
  );

  log(`等 PR #${prNumber} 的检查（CI 三个必需检查；docker 视改动范围可能被跳过）`);
  const res = exec(
    'gh',
    ['pr', 'checks', String(prNumber), '--watch', '--fail-fast', '--interval', '30'],
    { timeoutMs: 120 * 60_000 },
  );
  if (!res.ok) {
    // `gh pr checks` 的失败清单已经在上面的输出里；这里补一句下一步该做什么。
    fail(
      `PR #${prNumber} 的检查失败（退出码 ${res.status}）`,
      `看上面的失败作业名；修好后 commit + push，再重跑 npm run release（分支已存在，需先删）`,
    );
  }
}

function mergePr(prNumber) {
  log(`合并 PR #${prNumber}（rebase，保留 chore(release) 提交正文）`);
  must('gh', ['pr', 'merge', String(prNumber), '--rebase', '--delete-branch']);
  must('git', ['checkout', 'main']);
  must('git', ['pull', '--ff-only', 'origin', 'main']);
}

/** 取合并后的提交、打注解标签、推标签。注解标签才有 `Simple Post v…` 这条说明。 */
function tagRelease(next) {
  const sha = must('git', ['rev-parse', 'HEAD'], { capture: true }).stdout.trim();
  const subject = must('git', ['log', '-1', '--pretty=format:%s'], { capture: true }).stdout.trim();
  if (subject !== `chore(release): ${next}`) {
    fail(
      `合并后的 HEAD 是「${subject}」，不是 chore(release): ${next}`,
      '有人在这期间推了别的提交？确认 main 上那个提交确实是这次的版本号提升',
    );
  }
  must('git', ['tag', '-a', `v${next}`, sha, '-m', `Simple Post v${next}`]);
  must('git', ['push', 'origin', `v${next}`]);
  return { sha, tag: `v${next}` };
}

// ── 流水线与草案（--publish） ──────────────────────────────────────────────

/** 找这个标签 / 这个提交对应的 release.yml 运行。返回 null 表示「还没有」。 */
function findRun(tag, sha) {
  const res = exec(
    'gh',
    [
      'run',
      'list',
      '--workflow=release.yml',
      '--limit',
      '30',
      '--json',
      'databaseId,headBranch,headSha,status,conclusion,event',
    ],
    { capture: true },
  );
  if (!res.ok) return null;
  let runs;
  try {
    runs = JSON.parse(res.stdout);
  } catch {
    return null;
  }
  if (!Array.isArray(runs)) return null;
  return runs.find((run) => run.headSha === sha || run.headBranch === tag) ?? null;
}

/**
 * 等这次发布全部作业成功。
 *
 * 判据是「整轮成功」而不是「三个矩阵作业成功」：`fix-updater-json` 是后置作业，
 * 它没跑完就公布，用户的应用内更新会拿到 `403`（见文件头）。故这里额外确认存在
 * 一个叫 `fix-updater-json` 的作业且它成功 —— 那条 `needs: release` 若被人删掉，
 * 整轮也照样是绿的，只有这个名字能察觉。
 */
async function awaitWorkflow(tag, sha) {
  log(`等 release.yml 起作业（标签 ${tag}）`);
  const run = await poll(
    `release.yml 的运行（${tag}）`,
    () => {
      const found = findRun(tag, sha);
      return found ? found : null;
    },
    {
      timeoutMs: RUN_APPEAR_TIMEOUT_MS,
      hint: '检查 gh 权限（需 repo + workflow）、标签是不是真的推上去了：gh run list --workflow=release.yml',
    },
  );
  const runId = run.databaseId;
  log(`运行 ${runId} 已出现，等它全部作业结束（矩阵构建通常 20-40 分钟）`);

  const finished = await poll(
    `release.yml 运行 ${runId} 结束`,
    () => {
      const res = exec('gh', ['run', 'view', String(runId), '--json', 'status,conclusion,jobs'], {
        capture: true,
      });
      if (!res.ok) return null;
      let view;
      try {
        view = JSON.parse(res.stdout);
      } catch {
        return null;
      }
      return view.status === 'completed' ? view : null;
    },
    {
      timeoutMs: RUN_TIMEOUT_MS,
      hint: `看进度：gh run view ${runId}；失败的话先修再重发（标签已存在，不能重打）`,
    },
  );

  const jobs = Array.isArray(finished.jobs) ? finished.jobs : [];
  const failed = jobs.filter((job) => job.conclusion && job.conclusion !== 'success');
  if (finished.conclusion !== 'success' || failed.length > 0) {
    const names = failed.map((job) => `${job.name}（${job.conclusion}）`);
    fail(
      `release.yml 没有全部成功：${finished.conclusion ?? '未知'}${
        names.length > 0 ? `\n  失败的作业：${names.join('、')}` : ''
      }`,
      '本次不公布草案。修好问题后按 AGENTS.md 的发版段落人工收尾',
    );
  }

  const fixup = jobs.find((job) => job.name === 'fix-updater-json');
  if (!fixup || fixup.conclusion !== 'success') {
    fail(
      'release.yml 整轮是绿的，但没有成功的 `fix-updater-json` 作业',
      'latest.json 里可能还是 api.github.com 的资产端点（应用内更新会 403）；检查 release.yml 的后置作业',
    );
  }
  log('全部作业成功，含 fix-updater-json —— 现在公布才不会让应用内更新拿到 403');
}

/**
 * 等草案出现，把生成的改动小节嵌进它现有的正文（安装说明要留着）。
 *
 * `--publish` 与手动收尾共用这一份 —— 两条路径对同一份草案必须给出同一个正文。
 * 少了它，手动收尾会把 `release.yml` 写进草案的安装说明整段冲掉。
 */
async function draftNotes(tag, titles, next) {
  const draft = await poll(
    `草案 ${tag}`,
    () => {
      const res = exec('gh', ['release', 'view', tag, '--json', 'isDraft,body,url'], {
        capture: true,
      });
      if (!res.ok) return null;
      try {
        return JSON.parse(res.stdout);
      } catch {
        return null;
      }
    },
    {
      timeoutMs: DRAFT_TIMEOUT_MS,
      hint: `release.yml 成功但没有草案？看 gh release list 与那个运行的日志（gh run view <id> --log）`,
    },
  );

  const section = renderReleaseSection(titles);
  const notes = draft.body
    ? mergeReleaseBody(draft.body, section)
    : renderReleaseNotes(titles, next);
  return { notes, draft };
}

async function publishRelease(tag, titles, next, { publish }) {
  const { notes, draft } = await draftNotes(tag, titles, next);

  const dir = mkdtempSync(join(tmpdir(), 'simple-post-release-'));
  const file = join(dir, 'notes.md');
  try {
    writeFileSync(file, notes);
    const args = ['release', 'edit', tag, '--notes-file', file];
    if (publish) args.push('--draft=false');
    must('gh', args);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }

  log(publish ? `已公布 ${tag}` : `草案 ${tag} 的说明已填好（仍是草案）`);
  if (publish)
    log(`发布页：${draft.url ?? `https://github.com/Nnyjk/simple-post/releases/tag/${tag}`}`);
}

// ── dry-run ────────────────────────────────────────────────────────────────

function printPlan({ current, next, bump, branch, tag, prevTag, publish }) {
  const notes = renderReleaseNotes(readTitles(`${prevTag}..HEAD`), next);
  const steps = [
    'git fetch --tags origin',
    'git checkout -b ' + branch,
    `# 改写 ${VERSION_SOURCES.map((s) => s.file).join('、')}`,
    'cargo metadata --format-version 1',
    'npm install --package-lock-only',
    'git diff --stat HEAD   # 只许出现预期六个文件，锁文件只许动版本行',
    `git add ${EXPECTED_FILES.join(' ')}`,
    `git commit -m "chore(release): ${next}"`,
    `git push -u origin ${branch}`,
    `gh pr create --base main --head ${branch} --title "chore(release): ${next}" --body <发布说明>`,
    'gh pr checks <PR> --watch --fail-fast --interval 30',
    'gh pr merge <PR> --rebase --delete-branch',
    'git checkout main && git pull --ff-only origin main',
    `git tag -a ${tag} <合并后的提交> -m "Simple Post ${tag}"`,
    `git push origin ${tag}`,
  ];
  if (publish) {
    steps.push(
      'gh run list --workflow=release.yml --json databaseId,headBranch,headSha,status,conclusion',
      'gh run view <id> --json status,conclusion,jobs   # 等到 completed 且全部作业 success',
      `gh release view ${tag} --json isDraft,body,url  # 等到草案存在`,
      `gh release edit ${tag} --notes-file <tmp> --draft=false`,
    );
  } else {
    steps.push(
      `# 不公布。等 release.yml（含 fix-updater-json）成功后再手动 gh release edit ${tag} --notes-file <file> --draft=false`,
    );
  }

  const label = bump === 'patch' ? 'patch（末位 +1）' : `显式版本 ${bump}（逃生口）`;
  console.log('');
  log(`当前 ${current} → 目标 ${next}（${label}）`);
  log(`分支 ${branch}；标签 ${tag}；上一个标签 ${prevTag}`);
  log('dry-run：不建分支、不提交、不打标签、不调 gh 写命令。');
  console.log('\n将修改：');
  for (const source of VERSION_SOURCES) {
    console.log(`  ${source.file.padEnd(28)} ${current} → ${next}   （${source.describe}）`);
  }
  console.log(
    `  ${'Cargo.lock'.padEnd(28)} cargo metadata --format-version 1 重写工作区 crate 的版本行`,
  );
  console.log(`  ${'package-lock.json'.padEnd(28)} npm install --package-lock-only 重写根包版本`);
  console.log('\n将执行：');
  for (const step of steps) console.log(`  ${step}`);
  console.log(`\n发布说明（${prevTag}..HEAD 的提交标题）：\n`);
  console.log(notes);
}

// ── main ───────────────────────────────────────────────────────────────────

async function main() {
  let args;
  try {
    args = parseArgs(argvWithNpmConfig(process.argv.slice(2)));
  } catch (err) {
    fail(err.message, USAGE);
  }
  if (args.help) {
    console.log(USAGE);
    return;
  }

  const current = readCurrentVersion();
  let next;
  try {
    next = resolveNextVersion(current, args.bump);
  } catch (err) {
    fail(err.message, USAGE);
  }

  const prevTag = `v${current}`;
  const tag = `v${next}`;
  const branch = `chore/release-${next}`;

  if (args.dryRun) {
    inspectVersionSources(current);
    if (!exec('git', ['rev-parse', '--verify', '--quiet', `refs/tags/${prevTag}`]).ok) {
      fail(`上一个版本的标签 ${prevTag} 不存在`, '发布说明的区间就是它，缺了就没法生成');
    }
    const dirty = exec('git', ['status', '--porcelain'], { capture: true }).stdout.trim();
    if (dirty.length > 0) warn(`工作区有未提交的改动，dry-run 的计划里不含它们：\n${dirty}`);
    printPlan({ current, next, bump: args.bump, branch, tag, prevTag, publish: args.publish });
    return;
  }

  log(`${current} → ${next}（${tag}）`);
  assertPreconditions();
  if (!exec('git', ['rev-parse', '--verify', '--quiet', `refs/tags/${prevTag}`]).ok) {
    fail(`上一个版本的标签 ${prevTag} 不存在`, '发布说明的区间就是它，缺了就没法生成');
  }
  assertFreeName(branch, tag);

  log(`创建分支 ${branch}`);
  must('git', ['checkout', '-b', branch]);

  log('改写四处版本号');
  applyVersionSources(current, next);

  log('刷新 Cargo.lock 与 package-lock.json');
  refreshLockfiles();

  verifyDiff(current, next);

  must('git', ['add', ...EXPECTED_FILES]);
  must('git', ['commit', '-m', `chore(release): ${next}`]);
  must('git', ['push', '-u', 'origin', branch]);

  const titles = readTitles(`${prevTag}..HEAD`);
  const pr = createPr(branch, next, titles, current);
  log(`PR #${pr.number}：${pr.url}`);
  awaitChecks(pr.number);
  mergePr(pr.number);

  const { tag: pushedTag } = tagRelease(next);

  if (args.publish) {
    await awaitWorkflow(
      pushedTag,
      must('git', ['rev-parse', pushedTag], { capture: true }).stdout.trim(),
    );
    const releasedTitles = readTitles(`${prevTag}..${pushedTag}`);
    await publishRelease(pushedTag, releasedTitles, next, { publish: true });
    return;
  }

  // 手动收尾也走 `draftNotes`：只打印未合并的说明，操作者手抄一遍就等于把草案里的安装
  // 说明整段丢掉。写成文件（`git status` 看得见、不在 `.scratch/`），再把命令指向它。
  const notes = readTitles(`${prevTag}..${pushedTag}`);
  const { notes: merged } = await draftNotes(pushedTag, notes, next);
  const file = join(ROOT, `release-notes-${next}.md`);
  writeFileSync(file, merged);

  console.log('');
  log(`${pushedTag} 已推送，release.yml 正在构建（含 fix-updater-json）。`);
  log('草案不会自己公开 —— 等它全部作业成功后再公布，否则应用内更新会拿到 403：');
  console.log('  gh run list --workflow=release.yml --limit 5');
  console.log(`\n草案正文已合并好并写到：${file}`);
  console.log('（草案里的安装说明与容器镜像段落都在里面；核对后执行下面这条，然后删掉这个文件）\n');
  console.log(`  gh release edit ${pushedTag} --notes-file "${file}" --draft=false`);
}

main().catch((err) => {
  fail(err instanceof Error ? err.message : String(err));
});
