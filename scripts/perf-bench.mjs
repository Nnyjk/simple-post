#!/usr/bin/env node
// 测量 serve-web HTTP API 的 p50/p95/p99 延迟分位数。
//
// 用法示例：
//   node scripts/perf-bench.mjs [--target URL] [--n N] [--assert] # 指定目标与样本数并断言
// 默认目标为 http://127.0.0.1:4173，通过 --target 可指向其他实例；
// --assert 开启 p95 < 200ms 预算门控（超标非零退出）。
// 默认探测无载荷无鉴权的 /api/healthz 路由，测量纯网络与 axum 开销。

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

// 预热 3 次以消除 TCP 慢启动、首轮 JIT 与 SQLite 初始连接对分位数的干扰。
for (let i = 0; i < 3; i++) {
  try {
    await fetch(`${TARGET}${ROUTE}`, { headers });
  } catch {
    /* ignore */
  }
}

// ── 压测主循环 ────────────────────────────────────────────────────────────
const samples = [];
let failures = 0;
for (let i = 0; i < N; i++) {
  const t0 = performance.now();
  let status = 0;
  try {
    const r = await fetch(`${TARGET}${ROUTE}`, { headers });
    status = r.status;
    // 排空响应体以便连接复用。
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
