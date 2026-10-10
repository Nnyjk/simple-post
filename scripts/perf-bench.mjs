#!/usr/bin/env node
// scripts/perf-bench.mjs — measure p50/p95/p99 for the serve-web API.
//
//   node scripts/perf-bench.mjs [--target URL] [--n N] [--assert]
//
// By default hits http://127.0.0.1:4173 (the dev port from
// `npm run dev:split`); --target points the bench at any other running
// instance (e.g. one behind an nginx reverse proxy).
// --assert enforces the budget: p95 < 200ms
// and exits non-zero on regression.
//
// The script is silent (no JSON body, no auth) so it measures pure
// network + axum overhead, not SQLite. The /api/healthz handler is the
// truest public probe — it touches the DB pool but not a real query.
//
// A second mode `--route /api/projects` lets you bench the CRUD path;
// supply SIMPLE_POST_WEB_PASSWORD if your binary is locked.

import { performance } from 'node:perf_hooks';
import process from 'node:process';

const args = parseArgs(process.argv.slice(2));
const TARGET = (args.target ?? 'http://127.0.0.1:4173').replace(/\/+$/, '');
const ROUTE = args.route ?? '/api/healthz';
const N = Number.parseInt(args.n ?? '200', 10);
const ASSERT = Boolean(args.assert);
const PASSWORD = args.password ?? process.env.SIMPLE_POST_WEB_PASSWORD;

const headers = {};
if (PASSWORD) headers.authorization = `Bearer ${PASSWORD}`;

console.log(`[perf-bench] target=${TARGET} route=${ROUTE} n=${N} assert=${ASSERT}`);

// ----- Warm up -----------------------------------------------------------------
// Three throwaway hits before timing so TCP slow-start / first-bucket JIT
// / SQLite open don't poison the percentiles.
for (let i = 0; i < 3; i++) {
  try {
    await fetch(`${TARGET}${ROUTE}`, { headers });
  } catch {
    /* ignore */
  }
}

// ----- Main loop ------------------------------------------------------------
const samples = [];
let failures = 0;
for (let i = 0; i < N; i++) {
  const t0 = performance.now();
  let status = 0;
  try {
    const r = await fetch(`${TARGET}${ROUTE}`, { headers });
    status = r.status;
    // Drain body so connection can be pooled / reused.
    await r.arrayBuffer();
  } catch {
    /* transport error */
  }
  const t1 = performance.now();
  samples.push(t1 - t0);
  if (status === 0 || status >= 500) failures++;
}

if (failures > 0) {
  console.error(`[perf-bench] ${failures}/${N} requests failed (status=0 or 5xx)`);
  process.exit(1);
}

// ----- Percentiles ----------------------------------------------------------
samples.sort((a, b) => a - b);
const pct = (p) => samples[Math.min(samples.length - 1, Math.floor((p / 100) * samples.length))];
const p50 = pct(50);
const p95 = pct(95);
const p99 = pct(99);
const min = samples[0];
const max = samples[samples.length - 1];
const mean = samples.reduce((a, b) => a + b, 0) / samples.length;

const fmt = (ms) => ms.toFixed(2).padStart(7);
console.log(
  `[perf-bench] min=${fmt(min)}ms  p50=${fmt(p50)}ms  p95=${fmt(p95)}ms  p99=${fmt(p99)}ms  max=${fmt(max)}ms  mean=${fmt(mean)}ms`,
);

if (ASSERT) {
  const BUDGET_P95_MS = 200;
  if (p95 > BUDGET_P95_MS) {
    console.error(`[perf-bench] FAIL: p95 ${p95.toFixed(2)}ms exceeds budget ${BUDGET_P95_MS}ms`);
    process.exit(1);
  }
  console.log(`[perf-bench] PASS: p95 ${p95.toFixed(2)}ms within budget ${BUDGET_P95_MS}ms`);
}

// ----- Helpers --------------------------------------------------------------
function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith('--')) {
      const key = a.slice(2);
      const next = argv[i + 1];
      if (next && !next.startsWith('--')) {
        out[key] = next;
        i++;
      } else {
        out[key] = 'true';
      }
    }
  }
  return out;
}
