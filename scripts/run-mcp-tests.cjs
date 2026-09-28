#!/usr/bin/env node
// MCP 集成测试门控运行器：先通过统一解析器检查产物，再运行集成测试。
// 缺少产物、子进程启动失败或超时都会明确返回非零状态。

const path = require('path');
const { pathToFileURL } = require('node:url');

const ROOT = path.resolve(__dirname, '..');
const WALL_MS = 80_000;
const CLEANUP_MS = 2_000;

async function main() {
  const resolverUrl = pathToFileURL(
    path.join(ROOT, 'scripts', 'e2e', 'artifact-resolver.mjs'),
  ).href;
  const lifecycleUrl = pathToFileURL(
    path.join(ROOT, 'scripts', 'e2e', 'process-lifecycle.mjs'),
  ).href;
  const { requireWorkspaceBinary } = await import(resolverUrl);
  const { spawnTracked } = await import(lifecycleUrl);

  try {
    await requireWorkspaceBinary('simple-post', { profiles: ['debug', 'release'] });
  } catch (error) {
    console.error(`[build:tests] E2E 产物预检失败：${error.message}`);
    return 2;
  }

  const script = path.join(ROOT, 'scripts', 'mcp-integration.test.mjs');
  console.log('\n[build:tests] === mcp ===');
  const child = spawnTracked(
    process.execPath,
    [script],
    { stdio: ['ignore', 'pipe', 'pipe'] },
    {
      label: 'mcp integration',
      onStdout: (chunk) => process.stdout.write(chunk),
      onStderr: (chunk) => process.stderr.write(chunk),
    },
  );

  const exit = await child.waitForExit(WALL_MS);
  if (exit.timedOut) {
    console.error(`[build:tests] mcp 集成测试超过 ${WALL_MS / 1000}s，准备终止子进程。`);
    const cleanup = await child.terminate({
      gracefulMs: CLEANUP_MS,
      forceMs: CLEANUP_MS,
      closeMs: CLEANUP_MS,
    });
    console.error(`[build:tests] 超时清理结果：${JSON.stringify(cleanup)}`);
    return 124;
  }

  if (exit.spawnError) {
    console.error(`[build:tests] mcp 集成测试启动失败：${exit.spawnError.message}`);
    return 1;
  }

  const close = await child.waitForClose(CLEANUP_MS);
  if (close.timedOut) {
    console.error('[build:tests] mcp 集成测试输出管道未在期限内关闭。');
    return 1;
  }

  const code = exit.exitResult?.code ?? 1;
  if (code === 0) {
    console.log('\n[build:tests] MCP 集成测试通过');
  } else {
    console.error(
      `[build:tests] mcp 集成测试失败（exit ${code}${exit.exitResult?.signal ? `，signal ${exit.exitResult.signal}` : ''}）。`,
    );
    if (exit.stderrTail) {
      console.error(`[build:tests] stderr 尾部：\n${exit.stderrTail}`);
    }
  }
  return code;
}

main()
  .then((code) => {
    process.exitCode = code;
  })
  .catch((error) => {
    console.error(`[build:tests] 包装器异常：${error && error.stack ? error.stack : error}`);
    process.exitCode = 1;
  });
