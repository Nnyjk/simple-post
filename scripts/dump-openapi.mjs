#!/usr/bin/env node
// scripts/dump-openapi.mjs
//
// Generate an OpenAPI 3.1 document from the canonical
// `public_route_specs()` table in
// `crates/simple-post-server/src/web_api/spec/table.rs`.
//
// Why parse Rust source instead of calling the binary?
//   - The OpenAPI doc should stay valid even when the Rust crate fails
//     to compile (e.g. after a half-merged commit, or during CI
//     docs-only builds).
//   - It keeps the dumper free of any Rust toolchain dependency.
//
// The dumper:
//   1. Locates the `public_route_specs()` table anywhere under
//      `crates/simple-post-server/src/web_api{,/**.rs}` and greps the
//      `RouteSpec { ... }` literal rows in it.
//   2. Groups rows by path (one path may have multiple methods).
//   3. Emits an OpenAPI 3.1 yaml document with `paths` keyed by path.
//   4. Tags internal / omitted routes via a top-level note.
//
// The table may be one inline `vec![…]` inside `public_route_specs()`, or
// one `routes()` call per resource group (each group in its own file). The
// dumper reads the group names from the call order, so the emitted document
// keeps the table's own order in either shape.
//
// Usage:
//   node scripts/dump-openapi.mjs > openapi.yaml
//
// Caveat:
//   - This dumper trusts the literal shape of the `RouteSpec` table —
//     a Rust refactor that breaks the table (different field names,
//     different row layout) will silently emit empty yaml.
//   - The `summary` text is human-readable; the dumper does no
//     schema synthesis.

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, basename, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');
// web_api moved to the headless server crate when the Tauri split happened;
// the table moved with it.
const SRC_DIR = join(ROOT, 'crates', 'simple-post-server', 'src');
const WEB_API_FILE = join(SRC_DIR, 'web_api.rs');
const WEB_API_DIR = join(SRC_DIR, 'web_api');

const SPEC_FN = 'pub fn public_route_specs() -> Vec<RouteSpec>';

/**
 * Every Rust source under the `web_api/` module tree.
 *
 * The tree is walked rather than hard-coded, so the table may live in a flat
 * `web_api.rs` sibling or in any file below `web_api/`.
 */
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

/**
 * Resolve the Rust source that owns `public_route_specs()`.
 *
 * Walk the `web_api/` module tree and return the file holding the table.
 * A flat `web_api.rs` sibling is also accepted, since Rust resolves `mod
 * web_api;` to either layout.
 */
function findSpecSource() {
  for (const path of allRustSources()) {
    const text = readFileSync(path, 'utf8');
    if (text.includes(SPEC_FN)) return { path, text };
  }
  throw new Error(
    'public_route_specs() not found in crates/simple-post-server/src/web_api{.rs,/} — has the function been renamed?',
  );
}

/** Brace-match the `{ … }` block whose opening brace is the first one at or after `from`. */
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

// ---------------------------------------------------------------------------
// 1. Parse `public_route_specs()` from the web_api module.
// ---------------------------------------------------------------------------
const { text: src } = findSpecSource();

// Find the start of `pub fn public_route_specs() -> Vec<RouteSpec> {`
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

/**
 * The table has two shapes, and the dumper accepts both.
 *
 * Inline: `public_route_specs()` returns one `vec![ RouteSpec { … }, … ]`.
 * Split:  `public_route_specs()` calls one `routes()` per resource group, and
 *         each group lives in its own file (`…::routes()`).
 *
 * The group names are read from the call order in `public_route_specs()` so
 * the emitted document keeps the table's own order.
 */
const groupRe = /([A-Za-z_][A-Za-z0-9_]*)\s*::\s*routes\s*\(\s*\)/g;
const groupNames = [];
let gm;
while ((gm = groupRe.exec(body)) !== null) groupNames.push(gm[1]);

/** The body of the `routes()` function that the given group name points at. */
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

/**
 * The body of one `RouteSpec { … }` row.
 *
 * A row's `path` may contain `{param}` placeholders, so the body cannot be
 * matched with a brace-free character class — that drops every row whose path
 * carries a parameter. Scan from the opening brace and count braces outside
 * string literals instead.
 */
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

// Match each `RouteSpec { ... }` row.
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
      stability: get('stability'),
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

// A row that fails to parse would vanish from the document without a trace,
// so the dumper checks that every `RouteSpec {` in the source became a row.
const sourceRowCount = sourceBlocks.reduce(
  (n, b) => n + (b.match(/RouteSpec\s*\{/g) || []).length,
  0,
);
if (rows.length !== sourceRowCount) {
  throw new Error(
    `parsed ${rows.length} RouteSpec rows but the source holds ${sourceRowCount} — a row was dropped`,
  );
}

// ---------------------------------------------------------------------------
// 2. Group rows by path. Emit yaml.
// ---------------------------------------------------------------------------

/** YAML string escape — only need to handle double-quotes and backslashes. */
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
  // axum 0.7 writes path params as `:id`; OpenAPI requires `{id}`.
  // Emitting the axum form verbatim produces a document that no OpenAPI
  // 3.1 validator accepts. This is a presentation-only transform — the
  // axum source keeps `:id`.
  const template = path.replace(/:([A-Za-z_][A-Za-z0-9_]*)/g, '{$1}');
  out.push(`  ${template}:`);
  // OpenAPI requires every `{param}` in a path template to be declared.
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
    out.push(`      x-stability: ${r.stability}`);
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
out.push('x-internal-routes-not-documented:');
out.push('  - GET    /api/mcp/status');
out.push('  - POST   /api/mcp/start');
out.push('  - POST   /api/mcp/stop');
out.push('  - POST   /api/mcp/restart');
out.push('  - GET    /api/mcp/proxy-path');
out.push('  - GET    /api/mcp/capability');
out.push('  - PUT    /api/mcp/capability');
out.push('  - GET    /api/mcp/capability/tools');
out.push('  - GET    /api/preferences');
out.push('  - PUT    /api/preferences');
out.push('x-shape-references:');
out.push('  note: |');
out.push('    Request / response shapes referenced via `x-request-shape` / `x-response-shape`');
out.push(
  '    are domain types defined in `crates/simple-post-core/src/domain.rs` and serialised with',
);
out.push('    `#[serde(rename_all = "camelCase")]`.');
out.push('    Worked curl examples: see the README `serve-web` section.');

process.stdout.write(out.join('\n') + '\n');
