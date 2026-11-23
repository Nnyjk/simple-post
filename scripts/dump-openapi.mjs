#!/usr/bin/env node
// 解析 `crates/simple-post-server/src/web_api/spec/table.rs`（或 web_api/ 树下）的
// `public_route_specs()` 表，渲染 OpenAPI 3.1 YAML 文档。
//
// 为什么解析 Rust 源码而非调用二进制：
//   - 即使 Rust 暂时编译失败（如半合并提交或仅文档构建 CI），生成仍可独立运行。
//   - 避免生成脚本依赖 Rust 编译工具链。
//
// 为什么不将生成物提交进仓库：
//   - `public_route_specs()` 表是唯一真值，生成物写入 `target/openapi.yaml`（已被 git 忽略），
//     门禁按需生成并校验，避免版本库中出现双份真值与状态分叉。
//
// 流程：
//   1. 遍历定位包含 `public_route_specs()` 的文件，提取所有 `RouteSpec { ... }` 字面量；
//   2. 按 path 聚合多 HTTP method；
//   3. 输出 OpenAPI 3.1 YAML 文档。
//
// 用法示例：
//   node scripts/dump-openapi.mjs > openapi.yaml              # 输出至标准输出
//   node scripts/dump-openapi.mjs --out target/openapi.yaml   # 输出并校验 paths 非空
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, basename, join, resolve } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');
// 模块随前后端拆分移至 server crate，表定义位于 crates/simple-post-server/src。
const SRC_DIR = join(ROOT, 'crates', 'simple-post-server', 'src');
const WEB_API_FILE = join(SRC_DIR, 'web_api.rs');
const WEB_API_DIR = join(SRC_DIR, 'web_api');

const SPEC_FN = 'pub fn public_route_specs() -> Vec<RouteSpec>';

/** 遍历 web_api/ 下所有 Rust 源文件。表可能在 web_api.rs 或 web_api/ 子目录中。 */
function allRustSources() {
  const out = [];
  if (existsSync(WEB_API_DIR)) {
    const walk = (dir) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const p = join(dir, entry.name);
        if (entry.isDirectory()) walk(p);
        else if (entry.name.endsWith('.rs')) out.push(p);
      }
    };
    walk(WEB_API_DIR);
  }
  if (existsSync(WEB_API_FILE)) out.push(WEB_API_FILE);
  return out;
}

/** 定位包含 public_route_specs() 的 Rust 源文件。 */
function findSpecSource() {
  for (const path of allRustSources()) {
    const text = readFileSync(path, 'utf8');
    if (text.includes(SPEC_FN)) return { path, text };
  }
  throw new Error(
    'public_route_specs() not found in crates/simple-post-server/src/web_api{.rs,/} — has the function been renamed?',
  );
}

/** 从 from 后的第一个 '{' 开始括号匹配，返回闭合代码块。 */
function blockAfter(text, from) {
  const open = text.indexOf('{', from);
  if (open === -1) return null;
  let depth = 0;
  for (let i = open; i < text.length; i++) {
    if (text[i] === '{') depth++;
    else if (text[i] === '}') {
      depth--;
      if (depth === 0) return text.slice(open, i + 1);
    }
  }
  return null;
}

// 1. 从 web_api 源码中解析 public_route_specs() 函数体。
const { text: src } = findSpecSource();

// 定位 `pub fn public_route_specs() -> Vec<RouteSpec> {` 起始位置。
const fnStart = src.indexOf(SPEC_FN);
if (fnStart === -1) {
  throw new Error(
    'public_route_specs() not found in the web_api source — has the function been renamed?',
  );
}

const body = blockAfter(src, fnStart);
if (!body) {
  throw new Error('could not locate end of public_route_specs() body');
}

/** 表支持两种组织形式：内联单个 vec![RouteSpec { ... }]，或各资源分组单独调用 routes()。按调用顺序保持原表排序。 */
const groupRe = /([A-Za-z_][A-Za-z0-9_]*)\s*::\s*routes\s*\(\s*\)/g;
const groupNames = [];
let gm;
while ((gm = groupRe.exec(body)) !== null) groupNames.push(gm[1]);

/** 获取指定分组名称对应的 routes() 函数体代码块。 */
function groupBody(name) {
  for (const path of allRustSources()) {
    if (basename(path, '.rs') !== name) continue;
    const text = readFileSync(path, 'utf8');
    const idx = text.indexOf('fn routes');
    if (idx === -1) continue;
    const b = blockAfter(text, idx);
    if (b) return b;
  }
  return null;
}

let sourceBlocks = [body];
if (groupNames.length > 0) {
  sourceBlocks = groupNames.map((name) => {
    const b = groupBody(name);
    if (!b) {
      throw new Error(
        `public_route_specs() calls \`${name}::routes()\`, but no file under web_api/ defines it`,
      );
    }
    return b;
  });
}

/** 解析单个 RouteSpec { ... } 结构体字面量的内容，处理字符串内的花括号转义。 */
function rowBody(text, from) {
  let depth = 1;
  let quote = null;
  for (let i = from; i < text.length; i += 1) {
    const ch = text[i];
    if (quote) {
      if (ch === '\\') {
        i += 1;
        continue;
      }
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      continue;
    }
    if (ch === '{') depth += 1;
    else if (ch === '}') {
      depth -= 1;
      if (depth === 0) return text.slice(from, i);
    }
  }
  return null;
}

// 提取所有 RouteSpec { ... } 行。
const rows = [];
for (const block of sourceBlocks) {
  const rowRe = /RouteSpec\s*\{/g;
  let m;
  while ((m = rowRe.exec(block)) !== null) {
    const fields = rowBody(block, m.index + m[0].length);
    if (fields === null) continue;
    const get = (name) => {
      const re = new RegExp(`${name}:\\s*"([^"]*)"`);
      const r = fields.match(re);
      return r ? r[1] : null;
    };
    const getOpt = (name) => {
      const re = new RegExp(`${name}:\\s*(Some\\("([^"]*)"\\)|None)`);
      const r = fields.match(re);
      if (!r) return null;
      if (r[1].startsWith('None')) return null;
      return r[2];
    };
    rows.push({
      method: get('method'),
      path: get('path'),
      summary: get('summary'),
      request: getOpt('request'),
      response: getOpt('response'),
    });
  }
}

if (rows.length === 0) {
  throw new Error(
    'no RouteSpec rows parsed from public_route_specs() — has the table layout changed?',
  );
}

// 校验提取行数与源码中的 RouteSpec { 出现次数一致，防止解析静默遗漏。
const sourceRowCount = sourceBlocks.reduce(
  (n, b) => n + (b.match(/RouteSpec\s*\{/g) || []).length,
  0,
);
if (rows.length !== sourceRowCount) {
  throw new Error(
    `parsed ${rows.length} RouteSpec rows but the source holds ${sourceRowCount} — a row was dropped`,
  );
}

// 2. 按路径分组并生成 YAML。

/** YAML 字符串转义（处理双引号与反斜杠）。 */
function y(s) {
  return `"${String(s).replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

const byPath = new Map();
for (const r of rows) {
  if (!byPath.has(r.path)) byPath.set(r.path, []);
  byPath.get(r.path).push(r);
}

const out = [];
out.push('openapi: 3.1.0');
out.push('info:');
out.push(`  title: ${y('simple-post HTTP API (stable v1 contract)')}`);
out.push(`  version: 0.2.0`);
out.push('  description: |');
out.push('    Stable HTTP API exposed by `simple-post serve-web`. Endpoints are');
out.push('    partitioned into two tiers: `public` (this document; covered by');
out.push('    the v1 contract) and `internal` (UI / supervisor use only; see the');
out.push('    `web_api/` source). Internal endpoints are intentionally absent');
out.push('    from this document — they may change without notice.');
out.push('servers:');
out.push('  - url: http://localhost:4173');
out.push('    description: Local dev (default port)');
out.push('  - url: http://{host}:4173');
out.push('    variables:');
out.push('      host:');
out.push('        default: localhost');
out.push('        description: Hostname or LAN IP of the simple-post serve-web instance');
out.push('paths:');
for (const [path, methods] of [...byPath.entries()].sort()) {
  // axum 0.7 路径参数写作 :id，OpenAPI 规范要求写作 {id}，仅在此处做展示层转换。
  const template = path.replace(/:([A-Za-z_][A-Za-z0-9_]*)/g, '{$1}');
  out.push(`  ${template}:`);
  // 提取路径模板中的 {param} 并补全 parameters 声明。
  const params = [
    ...new Set([...template.matchAll(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g)].map((m) => m[1])),
  ];
  if (params.length > 0) {
    out.push('    parameters:');
    for (const name of params) {
      out.push(`      - name: ${name}`);
      out.push('        in: path');
      out.push('        required: true');
      out.push('        schema:');
      out.push('          type: string');
    }
  }
  for (const r of methods) {
    out.push(`    ${r.method.toLowerCase()}:`);
    out.push(`      summary: ${y(r.summary)}`);
    if (r.request) {
      out.push(`      x-request-shape: ${y(r.request)}`);
    }
    if (r.response) {
      out.push(`      x-response-shape: ${y(r.response)}`);
    }
    out.push(`      responses:`);
    out.push(`        '200':`);
    out.push(`          description: OK`);
  }
}
out.push('components:');
out.push('  securitySchemes: {}');
// 内部路由清单由 server 侧维护：`crates/simple-post-server/src/web_api/spec/mod.rs` 的「internal (not in OpenAPI v1)」一节逐条列出那些端点，本脚本不复制它。
out.push('x-shape-references:');
out.push('  note: |');
out.push('    Request / response shapes referenced via `x-request-shape` / `x-response-shape`');
out.push(
  '    are domain types defined in `crates/simple-post-core/src/domain.rs` and serialised with',
);
out.push('    `#[serde(rename_all = "camelCase")]`.');
out.push('    Worked curl examples: see the README `serve-web` section.');

const yaml = out.join('\n') + '\n';

/**
 * `--out <file>`：写文件并**断言写出来的东西不是空壳**。
 *
 * 加这个开关的原因是本脚本此前只在手动跑时才存在：`package.json` 的
 * `lint:openapi` 跑的是 interchange 的 `export_sample`，两条产物谁也不看谁，
 * 于是「表被重构 → 本脚本静默吐空 yaml」这条风险一直没人碰到。开关落在
 * `lint:openapi` 里之后，空壳会让 `npm run check` 直接红。
 *
 * 目标路径按 cwd 解析（不是 `__dirname`），调用方决定写哪儿；仓库里的门禁写
 * `target/openapi.yaml` —— `target/` 已被 git 忽略，产物不会进版本库。
 */
const argv = process.argv.slice(2);
const outIdx = argv.findIndex((a) => a === '--out' || a === '-o');
if (outIdx === -1) {
  process.stdout.write(yaml);
} else {
  const target = argv[outIdx + 1];
  if (!target) {
    console.error('dump-openapi: --out requires a path');
    process.exit(2);
  }
  const file = resolve(process.cwd(), target);
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, yaml, 'utf8');

  const written = readFileSync(file, 'utf8');
  const lines = written.split(/\r?\n/);
  const pathsAt = lines.indexOf('paths:');
  const rows =
    pathsAt === -1 ? 0 : lines.slice(pathsAt + 1).filter((l) => /^ {2}\/api\//.test(l)).length;
  if (pathsAt === -1 || rows === 0) {
    console.error(
      `dump-openapi: ${target} 写出来了，但没有 paths（paths: 在第 ${pathsAt} 行，${rows} 条）—— RouteSpec 表的布局变了`,
    );
    process.exit(1);
  }
  console.error(`dump-openapi: ${target} — ${rows} 条路径`);
}
