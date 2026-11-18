#!/usr/bin/env node
/**
 * scripts/rewrite-updater-json.mjs
 *
 * 把 `latest.json` 里 `platforms.*.url` 的 GitHub API 资产端点改写成 release 的下载地址。
 *
 * 为什么需要：`tauri-action` 在 `releaseDraft: true` 时把资产表里的
 * `https://api.github.com/repos/<owner>/<repo>/releases/assets/<id>` 写进 latest.json。
 * 那个端点是 API 端点，不是下载端点：它要求请求带 `User-Agent`，
 * 不带时返回 `403 Forbidden`（正文 `Request forbidden by administrative rules`），
 * 而 tauri-plugin-updater 用的 reqwest 默认不发 `User-Agent`，
 * 所以 0.1.35 → 0.1.36 的应用内更新拿到 `403`。
 * 同一条请求带上 `Accept: application/octet-stream` 才 `302` 去文件；不带只返回资产元数据 JSON。
 * 正确的地址是资产的 `browser_download_url`。
 *
 * 这个文件有两半：
 *   1. `rewriteUpdaterJson` 是纯函数，无副作用、不读环境变量，供单测 import；
 *   2. 文件末尾的 CLI 用 `gh` 取资产表与 latest.json、改写后写回同一个 release。
 *
 * Usage:
 *   node scripts/rewrite-updater-json.mjs --tag v0.1.36 --repo Nnyjk/simple-post
 *   node scripts/rewrite-updater-json.mjs --tag refs/tags/v0.1.36 --repo Nnyjk/simple-post --dry-run
 */
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** `https://api.github.com/repos/<owner>/<repo>/releases/assets/<id>`，容忍末尾斜杠。 */
const API_ASSET_URL =
  /^https:\/\/api\.github\.com\/repos\/([^/]+)\/([^/]+)\/releases\/assets\/(\d+)\/?$/;

/** 写进 latest.json 的地址都落在这个前缀下，故已是这种写法的 url 一律原样保留。 */
const DOWNLOAD_PREFIX = 'https://github.com/';

/**
 * 改写 latest.json 里每个平台的 url，返回新对象，入参一个字节也不改。
 *
 * `release` 是 `gh api repos/<repo>/releases/tags/<tag>` 的响应：`assets` 提供 id →
 * `browser_download_url` 的对照，`tag_name` 在资产没有 `browser_download_url` 时用来拼地址。
 *
 * 认不出的 url（未知资产 id，或既不是 API 端点也不是 github.com 地址）直接抛错：
 * 静默留下一个坏 url，等于把 403 留给装下一版的用户。
 */
export function rewriteUpdaterJson(latest, release) {
  const assets = Array.isArray(release?.assets) ? release.assets : [];
  const tag = typeof release?.tag_name === 'string' ? release.tag_name : '';
  const platforms = {};

  for (const [key, entry] of Object.entries(latest.platforms)) {
    const url = entry?.url;
    if (typeof url !== 'string') {
      throw new Error(`platforms.${key} 没有 string 类型的 url，无法改写：${String(url)}`);
    }
    platforms[key] = { ...entry, url: rewritePlatformUrl(key, url, assets, tag) };
  }

  // signature / notes / version / pub_date 由展开原样带过去；platforms 里每一条只换 url。
  return { ...latest, platforms };
}

/** 一个平台 url 的改写：已是下载地址的原样返回，否则查资产表。 */
function rewritePlatformUrl(key, url, assets, tag) {
  if (url.startsWith(DOWNLOAD_PREFIX)) return url;

  const match = API_ASSET_URL.exec(url);
  if (match === null) {
    throw new Error(
      `platforms.${key} 的 url 既不是 API 资产端点也不是 github.com 地址，无法改写：${url}`,
    );
  }

  const [, owner, repo, id] = match;
  const asset = assets.find((item) => String(item?.id) === id);
  if (asset === undefined) {
    throw new Error(`platforms.${key} 的 url 指向未知资产 id ${id}，资产表里没有它：${url}`);
  }

  if (typeof asset.browser_download_url === 'string' && asset.browser_download_url !== '') {
    return asset.browser_download_url;
  }

  if (typeof asset.name === 'string' && asset.name !== '' && tag !== '') {
    return `${DOWNLOAD_PREFIX}${owner}/${repo}/releases/download/${tag}/${encodeURIComponent(asset.name)}`;
  }

  throw new Error(
    `platforms.${key} 的资产 ${id} 既没有 browser_download_url 也没有可拼的名字：${url}`,
  );
}

const USAGE = `用法：
  node scripts/rewrite-updater-json.mjs --tag <tag> --repo <owner/repo> [--dry-run]

  --tag       release 标签，可带 refs/tags/ 前缀（脚本自己去掉）
  --repo      owner/repo
  --dry-run   只把改写后的内容打到 stdout，不写回 release

需要环境里有 gh，且 GH_TOKEN（或已登录的 gh）能读资产表、下载与上传 release 资产。
`;

/** 用 gh 跑一条命令，返回它的 stdout。 */
function runGh(args) {
  return execFileSync('gh', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] });
}

/** 解析 CLI 参数；未知参数直接抛错，免得拼错一个开关就静默跑错分支。 */
function parseArgs(argv) {
  const out = { tag: '', repo: '', dryRun: false, help: false };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--dry-run') out.dryRun = true;
    else if (arg === '--help' || arg === '-h') out.help = true;
    else if (arg.startsWith('--tag=')) out.tag = arg.slice('--tag='.length);
    else if (arg.startsWith('--repo=')) out.repo = arg.slice('--repo='.length);
    else if (arg === '--tag') out.tag = argv[(i += 1)] ?? '';
    else if (arg === '--repo') out.repo = argv[(i += 1)] ?? '';
    else throw new Error(`未知参数：${arg}\n\n${USAGE}`);
  }

  return out;
}

function main(argv) {
  const { tag, repo, dryRun, help } = parseArgs(argv);

  if (help) {
    process.stdout.write(USAGE);
    return;
  }
  if (tag === '' || repo === '') {
    throw new Error(`--tag 与 --repo 都是必需的。\n\n${USAGE}`);
  }

  const bareTag = tag.replace(/^refs\/tags\//, '');
  const release = JSON.parse(runGh(['api', `repos/${repo}/releases/tags/${bareTag}`]));

  // 没有签名私钥时工作流关掉 `createUpdaterArtifacts`，这次发布不产 latest.json。
  // 那种发布里没有可改写的东西，跳过而不是报错，发布流水线要保持绿。
  const hasLatest =
    Array.isArray(release.assets) && release.assets.some((asset) => asset?.name === 'latest.json');
  if (!hasLatest) {
    process.stderr.write(
      `本次发布没有 latest.json（未配置签名私钥，createUpdaterArtifacts 被关掉），改写作业无事可做：${repo} ${bareTag}\n`,
    );
    return;
  }

  // latest.json 由 gh 下载到临时目录，改写后从同一个文件上传，故临时目录必须每次新建。
  const dir = mkdtempSync(join(tmpdir(), 'simple-post-latest-'));
  try {
    const file = join(dir, 'latest.json');
    runGh([
      'release',
      'download',
      bareTag,
      '-R',
      repo,
      '-p',
      'latest.json',
      '-D',
      dir,
      '--clobber',
    ]);

    const latest = JSON.parse(readFileSync(file, 'utf8'));
    const text = `${JSON.stringify(rewriteUpdaterJson(latest, release), null, 2)}\n`;

    if (dryRun) {
      process.stdout.write(text);
      return;
    }

    writeFileSync(file, text, 'utf8');
    runGh(['release', 'upload', bareTag, file, '-R', repo, '--clobber']);
    process.stderr.write(`latest.json 已改写并写回 ${repo} 的 ${bareTag}\n`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

// 被单测 import 时不跑 CLI。
const invokedDirectly =
  process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (invokedDirectly) {
  try {
    main(process.argv.slice(2));
  } catch (err) {
    process.stderr.write(`错误：${err instanceof Error ? err.message : String(err)}\n`);
    process.exitCode = 1;
  }
}
