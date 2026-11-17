import assert from 'node:assert/strict';
import { test } from 'node:test';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

/**
 * `scripts/` 的三向路径守卫。
 *
 * `scripts/` 下的东西不在 `npm test` 的采集范围里（`scripts/run-tests.mjs` 只走
 * `src/**\/*.test.ts`），eslint 也只查语法与未定义变量 —— 于是三条腐烂路径全都
 * 静默：npm script 指向一个已删的文件、某个脚本再没人调用也再没人删、脚本里写着
 * 一个已经不存在的 REST 路径。CI 绿着，脚本死了。
 *
 * 这里不去执行它们（它们要真端口、真二进制、真 Tauri 壳），只做静态可达性：
 *
 *   1. `package.json` 里每个 `node scripts/…` 的目标必须存在；
 *   2. `scripts/` 下每个 `.mjs`/`.cjs` 必须能从某个 npm script 出发、顺着相对
 *      import 走到（或被登记进 `STANDALONE` 并写清为什么）；反过来，`STANDALONE`
 *      里也不许留已经删掉的条目；
 *   3. 每个脚本里的 `/api/…`、`/healthz` 字面量必须是 `web-routes.ts` 的路径
 *      前缀之一（或登记进 `ALLOWED_LITERALS` 并写清理由）—— 这条与 Rust 侧的
 *      `route_specs` 守卫互补：那边守「表里有、router 里没有」，这边守「脚本里
 *      写着一个已经不存在的路径」。
 *
 * 刻意只做静态检查：不 spawn 进程、不连端口、不要求 `target/debug` 里二进制存在，
 * 所以它能留在 `npm test` 里，成为 `npm run check` 的一部分。
 */

const ROOT = resolve(process.cwd());
const SCRIPTS_DIR = join(ROOT, 'scripts');
const WEB_ROUTES_SRC = join(ROOT, 'src', 'lib', 'tauri-commands', 'web-routes.ts');

/**
 * 没有 npm script 入口、也没有被别的脚本 import 的脚本。
 *
 * 每一项都必须写一行「为什么它只手动跑」——这是这张表存在的唯一理由，写不出来
 * 就说明这个脚本该有个入口、或者该删掉。
 */
const STANDALONE: Record<string, string> = {
  'scripts/_verify_store.mjs':
    '手动排查工具（AGENTS.md:271/:472 记的 `npx tsx scripts/_verify_store.mjs`）：用 mockIPC 在纯 node 里跑 store，断言的是「store-extensions」那次计划的中间态其选择器与级联删除形状；放进快速门禁会因为 store 的正常重构频繁假红，故只手动跑。',
};

/**
 * 脚本里出现、但天然不在 `web-routes.ts` 里的路径。
 *
 * 健康探针不是命令，没有 Tauri 对应物，故不受 dual-transport 不变式约束。
 */
const ALLOWED_LITERALS: Record<string, string> = {
  '/api/healthz':
    '健康探针，注册在 `crates/simple-post-server/src/web_api/router.rs:73`，不是命令、无渲染层对应物。',
  '/healthz': '同上，无前缀别名（`router.rs:80`）；`auth::is_public_path` 名单里也有它。',
};

interface PackageJson {
  scripts: Record<string, string>;
}

const pkg = JSON.parse(readFileSync(join(ROOT, 'package.json'), 'utf8')) as PackageJson;

/** `scripts/` 下所有可执行脚本的绝对路径。 */
function collectScripts(dir: string = SCRIPTS_DIR, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) collectScripts(p, out);
    else if (/\.(mjs|cjs)$/.test(entry)) out.push(p);
  }
  return out;
}

/**
 * npm script 里 `node scripts/…` / `tsx scripts/…` 的目标，键为仓库相对路径。
 *
 * 值是第一个引用它的 script 名，失败信息里用得上。
 */
function npmScriptTargets(): Map<string, string> {
  const out = new Map<string, string>();
  for (const [name, command] of Object.entries(pkg.scripts)) {
    for (const m of command.matchAll(
      /(?:node|(?:npx\s+)?tsx)\s+(?:\.\/)?(scripts\/[\w./-]+\.(?:mjs|cjs|js))/g,
    )) {
      if (!out.has(m[1])) out.set(m[1], name);
    }
  }
  return out;
}

/** 一个脚本 import 的相对路径，解析成绝对路径（不做存在性判断）。 */
function relativeImports(file: string): string[] {
  const src = readFileSync(file, 'utf8');
  const out: string[] = [];
  for (const m of src.matchAll(/(?:from\s*|import\s*\(\s*)['"](\.[^'"]+)['"]/g)) {
    out.push(resolve(dirname(file), m[1]));
  }
  return out;
}

/**
 * `web-routes.ts` 里每条路由的静态路径前缀。
 *
 * 只取模板字面量的字面部分：`/api/nodes/${encodeURIComponent(...)}` → `/api/nodes`。
 * 前缀比完整模板更宽松（`/api/nodes/{id}/scripts` 也归到 `/api/nodes`），这正是想要的
 * —— 脚本里写的是具体路径，这里只判「它落在某条已注册路由的子树里」。
 */
function routeHeads(): string[] {
  const src = readFileSync(WEB_ROUTES_SRC, 'utf8');
  const heads = new Set<string>();
  for (const m of src.matchAll(/path:\s*\(?\w*\)?\s*=>\s*[`'"](\/api\/[^`'"]*)/g)) {
    const head = m[1].split('${')[0].replace(/\/+$/, '');
    if (head) heads.add(head);
  }
  return [...heads];
}

/**
 * 去掉 `//` 与 `/* … *\/` 注释，字符串与模板字面量原样保留。
 *
 * 必需：注释里的反引号（`例如 `/api/...` 这种说明`）会被「找模板字面量」的正则
 * 当成真字符串，于是纯注释能凭空造出一条待检查的路径。
 */
function stripComments(src: string): string {
  let out = '';
  let i = 0;
  let quote: string | null = null;
  while (i < src.length) {
    const ch = src[i];
    if (quote !== null) {
      out += ch;
      if (ch === '\\') {
        out += src[i + 1] ?? '';
        i += 2;
        continue;
      }
      if (ch === quote) quote = null;
      i += 1;
      continue;
    }
    if (ch === '/' && src[i + 1] === '/') {
      while (i < src.length && src[i] !== '\n') i += 1;
      continue;
    }
    if (ch === '/' && src[i + 1] === '*') {
      i += 2;
      while (i < src.length && !(src[i] === '*' && src[i + 1] === '/')) i += 1;
      i += 2;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === '`') quote = ch;
    out += ch;
    i += 1;
  }
  return out;
}

/**
 * 脚本源码里的字符串字面量中出现的 API 路径。
 *
 * 只看字面量、不看注释：`/api/...` 在注释里是说明，在字符串里是请求。模板字面量按
 * `${…}` 切开，取以 `/api/`（或 `/healthz`）开头的片段 —— 于是
 * `` `${BASE}${PREFIX}/api/projects` `` 归一成 `/api/projects`，而
 * `'@tauri-apps/api/mocks'` 这类模块标识符被丢弃。
 */
function apiLiterals(source: string): string[] {
  const src = stripComments(source);
  const out = new Set<string>();
  const literals: string[] = [];
  for (const m of src.matchAll(/`([^`]*)`/g)) literals.push(m[1]);
  for (const m of src.matchAll(/'([^'\n]*)'/g)) literals.push(m[1]);
  for (const m of src.matchAll(/"([^"\n]*)"/g)) literals.push(m[1]);

  for (const literal of literals) {
    for (const piece of literal.split(/\$\{[^}]*\}/)) {
      const clean = piece.trim().split(/[?#]/)[0].replace(/\/+$/, '');
      if (!clean.startsWith('/api/') && clean !== '/healthz') continue;
      if (!/^\/[A-Za-z0-9/_{}.-]+$/.test(clean)) continue;
      // 文档里的省略写法（`/api/...`）不是路径。
      if (clean.includes('...')) continue;
      out.add(clean);
    }
  }
  return [...out];
}

test('每个 npm script 指向的脚本文件都存在', () => {
  const missing: string[] = [];
  for (const [rel, name] of npmScriptTargets()) {
    if (!existsSync(join(ROOT, rel))) missing.push(`${rel}  ← npm script "${name}"`);
  }
  assert.deepEqual(missing, [], `npm scripts 指向了不存在的文件:\n${missing.join('\n')}`);
});

test('scripts/ 下每个脚本要么有 npm 入口，要么在 STANDALONE 里写明理由', () => {
  const files = collectScripts();
  const byPath = new Map(files.map((f) => [f, f]));
  const roots: string[] = [];
  for (const rel of npmScriptTargets().keys()) {
    const abs = resolve(ROOT, rel);
    if (byPath.has(abs)) roots.push(abs);
  }

  const reached = new Set<string>();
  const queue = [...roots];
  while (queue.length > 0) {
    const file = queue.pop() as string;
    if (reached.has(file)) continue;
    reached.add(file);
    for (const dep of relativeImports(file)) {
      if (byPath.has(dep) && !reached.has(dep)) queue.push(dep);
    }
  }

  const unrouted = files
    .filter((f) => !reached.has(f))
    .map((f) => f.slice(ROOT.length + 1).replaceAll('\\', '/'))
    .filter((rel) => !(rel in STANDALONE));

  assert.deepEqual(
    unrouted,
    [],
    `这些脚本没有 npm 入口、也没被别的脚本 import —— 加一个 npm script，或登记进 STANDALONE:\n${unrouted.join('\n')}`,
  );
});

test('STANDALONE 的每一项都还在、且写清了理由', () => {
  for (const [rel, reason] of Object.entries(STANDALONE)) {
    assert.ok(existsSync(join(ROOT, rel)), `STANDALONE 里的 ${rel} 已不存在 —— 删掉这一行`);
    assert.ok(reason.length >= 20, `STANDALONE 的 ${rel} 需要一行「为什么它只手动跑」`);
  }
});

test('每个脚本里的相对 import 都指向存在的文件', () => {
  const broken: string[] = [];
  for (const file of collectScripts()) {
    for (const dep of relativeImports(file)) {
      if (existsSync(dep)) continue;
      broken.push(`${file.slice(ROOT.length + 1)} → ${dep.slice(ROOT.length + 1)}`);
    }
  }
  assert.deepEqual(broken, [], `脚本 import 了已被删除的模块:\n${broken.join('\n')}`);
});

test('脚本里的 API 字面量都落在 web-routes.ts 的某条路由上', () => {
  const heads = routeHeads();
  // 46 条不同路径归并成 32 条静态前缀（`/api/nodes` 一条就覆盖 8 条命令）。
  // 阈值只用来抓「解析规则失效」这种断崖，不锁具体数字。
  assert.ok(
    heads.length > 20,
    `只从 web-routes.ts 解析出 ${heads.length} 条路径前缀 —— 解析规则失效了`,
  );

  const uncovered: string[] = [];
  for (const file of collectScripts()) {
    for (const literal of apiLiterals(readFileSync(file, 'utf8'))) {
      if (literal in ALLOWED_LITERALS) continue;
      const hit = heads.some((h) => literal === h || literal.startsWith(`${h}/`));
      if (!hit) uncovered.push(`${file.slice(ROOT.length + 1)} → ${literal}`);
    }
  }
  assert.deepEqual(
    uncovered,
    [],
    `这些请求路径在 web-routes.ts 里没有对应路由（改路由名后忘了改脚本，或脚本引用已删路由）:\n${uncovered.join('\n')}`,
  );
});
