#!/usr/bin/env node
// scripts/dump-openapi.mjs
//
// Generate an OpenAPI 3.1 document from the canonical
// `public_route_specs()` table in `src-tauri/src/web_api/spec.rs`.
//
// Why parse Rust source instead of calling the binary?
//   - The OpenAPI doc should stay valid even when the Rust crate fails
//     to compile (e.g. after a half-merged commit, or during CI
//     docs-only builds).
//   - It keeps the dumper free of any Rust toolchain dependency.
//
// The dumper:
//   1. Locates the `public_route_specs()` table anywhere under
//      `src-tauri/src/web_api{,/**.rs}` and greps the `RouteSpec { ... }`
//      literal rows in it.
//   2. Groups rows by path (one path may have multiple methods).
//   3. Emits an OpenAPI 3.1 yaml document with `paths` keyed by path.
//   4. Tags internal / omitted routes via a top-level note.
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
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');
const SRC_DIR = join(ROOT, 'src-tauri', 'src');
const WEB_API_FILE = join(SRC_DIR, 'web_api.rs');
const WEB_API_DIR = join(SRC_DIR, 'web_api');

const SPEC_FN = 'pub fn public_route_specs() -> Vec<RouteSpec>';

/**
 * Resolve the Rust source that owns `public_route_specs()`.
 *
 * Walk the `web_api/` module tree and return the file holding the table.
 * A flat `web_api.rs` sibling is also accepted, since Rust resolves `mod
 * web_api;` to either layout.
 */
function findSpecSource() {
  const candidates = [];
  if (existsSync(WEB_API_DIR)) {
    const walk = (dir) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const p = join(dir, entry.name);
        if (entry.isDirectory()) walk(p);
        else if (entry.name.endsWith('.rs')) candidates.push(p);
      }
    };
    walk(WEB_API_DIR);
  }
  if (existsSync(WEB_API_FILE)) candidates.push(WEB_API_FILE);

  for (const path of candidates) {
    const text = readFileSync(path, 'utf8');
    if (text.includes(SPEC_FN)) return text;
  }
  throw new Error(
    'public_route_specs() not found in src-tauri/src/web_api{.rs,/} — has the function been renamed?',
  );
}

// ---------------------------------------------------------------------------
// 1. Parse `public_route_specs()` from the web_api module.
// ---------------------------------------------------------------------------
const src = findSpecSource();

// Find the start of `pub fn public_route_specs() -> Vec<RouteSpec> {`
const fnStart = src.indexOf(SPEC_FN);
if (fnStart === -1) {
  throw new Error(
    'public_route_specs() not found in the web_api source — has the function been renamed?',
  );
}

// Match balanced `{ ... }` from the function start.
let depth = 0;
let i = src.indexOf('{', fnStart);
const bodyStart = i;
for (; i < src.length; i++) {
  if (src[i] === '{') depth++;
  if (src[i] === '}') depth--;
  if (depth === 0) break;
}
if (depth !== 0) {
  throw new Error('could not locate end of public_route_specs() body');
}
const body = src.slice(bodyStart, i + 1);

// Match each `RouteSpec { ... }` row (non-greedy, no nested braces).
const rowRe = /RouteSpec\s*\{([^{}]+)\}/g;
const rows = [];
let m;
while ((m = rowRe.exec(body)) !== null) {
  const fields = m[1];
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

if (rows.length === 0) {
  throw new Error(
    'no RouteSpec rows parsed from public_route_specs() — has the table layout changed?',
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
out.push('  - url: http://localhost:4224');
out.push('    description: Local dev (default port)');
out.push('  - url: http://{host}:4224');
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
  out.push(`  ${path.replace(/:([A-Za-z_][A-Za-z0-9_]*)/g, '{$1}')}:`);
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
out.push('    are domain types defined in `src-tauri/src/domain.rs` and serialised with');
out.push('    `#[serde(rename_all = "camelCase")]`.');
out.push('    Worked curl examples: see the README `serve-web` section.');

process.stdout.write(out.join('\n') + '\n');
