#!/usr/bin/env node
// 同时启动 Vite 开发服务器（端口 5173）与 Rust serve-web（端口 4173）。
// Vite 将 /api 请求代理至 127.0.0.1:4173（见 vite.config.ts 的 proxy 配置）。
//
// 进程说明：
//   [vite]      → `npm run dev`（端口 5173，提供前端 HMR）
//   [serve-web] → `cargo run … serve-web`（端口 4173，提供 REST 与 SPA 外壳）
//
// 约束与清理：
//   - serve-web 首次编译需 10–60 秒，未就绪前 /api 请求返回 502。
//   - 收到 SIGINT/SIGTERM 时向子进程发送终止信号，超时 3 秒后强制终止。Windows 下使用 `taskkill /T /F` 递归清理孙进程。
//   - 仅依赖 Node 原生模块，不引入第三方多进程依赖。

import { spawn } from 'node:child_process';
import process from 'node:process';
import { join } from 'node:path';

const REPO = join(import.meta.dirname, '..');
const VITE_PREFIX = '[vite] ';
const SERVE_PREFIX = '[serve-web] ';
const SHUTDOWN_GRACE_MS = 3000;

// 记录已启动的子进程句柄，用于信号与退出时统一清理。
const children = [];
let shuttingDown = false;
let forceKillTimer = null;
// 默认退出码为 0；任一子进程异常退出时写为非零值。
let exitCode = 0;

function prefixWrite(stream, chunk, prefix) {
  // 逐块添加前缀输出，避免按换行切分导致 stdout 与 stderr 错序。
  stream.write(prefix + chunk.toString());
}

function spawnChild(label, cmd, args) {
  const prefix = label === 'vite' ? VITE_PREFIX : SERVE_PREFIX;
  console.error(`[dev-split] starting ${label}: ${cmd} ${args.join(' ')}`);
  // Windows 下执行 .cmd/.bat 需开启 shell: true（Node 20+ 安全限制）；参数均为硬编码。
  const needsShell = process.platform === 'win32' && /\.(cmd|bat)$/i.test(cmd);
  const child = spawn(cmd, args, {
    cwd: REPO,
    env: process.env,
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true,
    shell: needsShell,
  });
  const handle = { label, child, prefix };
  children.push(handle);

  child.stdout.on('data', (b) => prefixWrite(process.stdout, b, prefix));
  child.stderr.on('data', (b) => prefixWrite(process.stderr, b, prefix));

  child.on('exit', (code, signal) => {
    if (shuttingDown) return; // 收到退出信号触发的主动关闭，忽略
    shuttingDown = true;
    if (code === 0 || signal === 'SIGTERM' || signal === 'SIGINT') {
      console.error(`[dev-split] ${label} exited cleanly (code=${code} signal=${signal})`);
    } else {
      console.error(`[dev-split] ${label} exited with code=${code} signal=${signal}`);
      exitCode = code ?? 1;
    }
    shutdownAll();
  });

  child.on('error', (err) => {
    // 启动失败立即报错退出，避免静默挂起。
    console.error(`[dev-split] ${label} failed to spawn: ${err.message}`);
    if (!shuttingDown) {
      shuttingDown = true;
      exitCode = 127;
      shutdownAll();
    }
  });

  return child;
}

// 向存活的子进程发送终止信号。
function shutdownAll() {
  for (const { child } of children) {
    if (child.exitCode !== null) continue;
    killTree(child, /* force = */ false);
  }
  if (forceKillTimer) clearTimeout(forceKillTimer);
  forceKillTimer = setTimeout(() => {
    for (const { label, child } of children) {
      if (child.exitCode === null) {
        console.error(
          `[dev-split] ${label} still alive after ${SHUTDOWN_GRACE_MS}ms — force killing`,
        );
        killTree(child, /* force = */ true);
      }
    }
    process.exit(exitCode || 0);
  }, SHUTDOWN_GRACE_MS);
}

// 跨平台递归清理子进程树。
function killTree(child, force) {
  if (!child || child.exitCode !== null) return;
  if (process.platform === 'win32') {
    // Windows 下用 /T 递归清理孙进程（cargo→simple-post.exe、npm.cmd→node.exe→vite），/F 强制终止。
    try {
      const killer = spawn('taskkill', ['/pid', String(child.pid), '/T', '/F'], {
        stdio: 'ignore',
        windowsHide: true,
      });
      killer.on('error', () => {
        /* 进程已退出，忽略错误 */
      });
    } catch {
      /* 进程已退出，忽略错误 */
    }
    return;
  }
  try {
    child.kill(force ? 'SIGKILL' : 'SIGTERM');
  } catch {
    /* 进程已退出，忽略错误 */
  }
}

process.on('SIGINT', () => {
  if (shuttingDown) return;
  shuttingDown = true;
  console.error('\n[dev-split] SIGINT received — shutting down children');
  shutdownAll();
});

process.on('SIGTERM', () => {
  if (shuttingDown) return;
  shuttingDown = true;
  console.error('\n[dev-split] SIGTERM received — shutting down children');
  shutdownAll();
});

// ----- 启动 -----------------------------------------------------------------
// 1. 优先启动 serve-web。服务绑定端口前 Vite 的 /api 代理会返回 502，axum 监听就绪后恢复。
console.error('[dev-split] hint: first cargo build of serve-web can take 10–60s');
spawnChild('serve-web', 'cargo', [
  'run',
  '--manifest-path',
  'src-tauri/Cargo.toml',
  '--bin',
  'simple-post',
  '--',
  'serve-web',
  '--bind',
  '127.0.0.1',
  '--port',
  '4173',
  '--spa-dir',
  './dist',
]);

// 2. 启动 Vite。Windows 下显式使用 npm.cmd。
const npmCmd = process.platform === 'win32' ? 'npm.cmd' : 'npm';
spawnChild('vite', npmCmd, ['run', 'dev']);
