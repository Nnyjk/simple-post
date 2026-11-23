#!/usr/bin/env node

import { spawn } from 'node:child_process';
import { rmSync } from 'node:fs';
import process from 'node:process';

const TAIL_BYTES = 8 * 1024;

function appendTail(current, chunk) {
  const next = current + chunk.toString();
  return next.length > TAIL_BYTES ? next.slice(-TAIL_BYTES) : next;
}

function hasExited(tracked) {
  return tracked.spawnError !== null || tracked.exitResult !== null;
}

function hasClosed(tracked) {
  return tracked.closeResult !== null;
}

function removeWaiter(waiters, waiter) {
  const index = waiters.indexOf(waiter);
  if (index >= 0) waiters.splice(index, 1);
}

export function trackChild(child, { label = 'child', onStdout, onStderr } = {}) {
  const tracked = {
    child,
    label,
    spawnError: null,
    exitResult: null,
    closeResult: null,
    stdoutTail: '',
    stderrTail: '',
    exitWaiters: [],
    closeWaiters: [],
  };

  const snapshot = (timedOut = false) => ({
    label,
    timedOut,
    spawnError: tracked.spawnError,
    exitResult: tracked.exitResult,
    closeResult: tracked.closeResult,
    stdoutTail: tracked.stdoutTail,
    stderrTail: tracked.stderrTail,
  });

  const settleExitWaiters = () => {
    if (!hasExited(tracked)) return;
    const result = snapshot(false);
    for (const waiter of tracked.exitWaiters.splice(0)) {
      clearTimeout(waiter.timer);
      waiter.resolve(result);
    }
  };

  const settleCloseWaiters = () => {
    if (!hasClosed(tracked)) return;
    const result = snapshot(false);
    for (const waiter of tracked.closeWaiters.splice(0)) {
      clearTimeout(waiter.timer);
      waiter.resolve(result);
    }
  };

  child.stdout?.on('data', (chunk) => {
    tracked.stdoutTail = appendTail(tracked.stdoutTail, chunk);
    onStdout?.(chunk);
  });
  child.stderr?.on('data', (chunk) => {
    tracked.stderrTail = appendTail(tracked.stderrTail, chunk);
    onStderr?.(chunk);
  });
  child.once('error', (error) => {
    tracked.spawnError = error;
    settleExitWaiters();
  });
  child.once('exit', (code, signal) => {
    tracked.exitResult = { code, signal };
    settleExitWaiters();
  });
  child.once('close', (code, signal) => {
    tracked.closeResult = { code, signal };
    settleCloseWaiters();
  });

  const waitFor = (waiters, ready, timeoutMs) => {
    if (ready()) return Promise.resolve(snapshot(false));
    return new Promise((resolve) => {
      const waiter = {
        resolve,
        timer: setTimeout(
          () => {
            removeWaiter(waiters, waiter);
            resolve(snapshot(true));
          },
          Math.max(0, timeoutMs),
        ),
      };
      waiters.push(waiter);
    });
  };

  tracked.waitForExit = (timeoutMs = 1_000) =>
    waitFor(tracked.exitWaiters, () => hasExited(tracked), timeoutMs);
  tracked.waitForClose = (timeoutMs = 1_000) =>
    waitFor(tracked.closeWaiters, () => hasClosed(tracked), timeoutMs);

  /**
   * 子进程退出后主动释放管道流与进程句柄。
   *
   * 防范 Windows 退出崩溃：流句柄若未显式销毁，libuv 在退出阶段会重复关闭句柄，
   * 触发断言失败（0xC0000409），导致所有通过的测试在结束时误报崩溃。
   */
  const releaseHandles = () => {
    for (const stream of [child.stdin, child.stdout, child.stderr]) {
      if (!stream || stream.destroyed) continue;
      try {
        stream.destroy();
      } catch {
        // 管道可能已被 Node 关闭，忽略。
      }
    }
    try {
      child.removeAllListeners();
      child.unref();
    } catch {
      // 子进程已完全回收，忽略。
    }
  };

  tracked.terminate = async ({ gracefulMs = 1_500, forceMs = 2_000, closeMs = 2_000 } = {}) => {
    if (!hasExited(tracked)) {
      try {
        child.kill(process.platform === 'win32' ? undefined : 'SIGTERM');
      } catch {
        // 子进程可能已经退出，继续等待其最终状态。
      }
      const graceful = await tracked.waitForExit(gracefulMs);
      if (graceful.timedOut && !hasExited(tracked)) {
        try {
          child.kill('SIGKILL');
        } catch {
          // 强制终止失败时仍返回超时状态，由调用方报告失败。
        }
        await tracked.waitForExit(forceMs);
      }
    }

    const close = await tracked.waitForClose(closeMs);
    releaseHandles();
    return snapshot(close.timedOut);
  };

  return tracked;
}

export function spawnTracked(command, args, options, trackOptions = {}) {
  return trackChild(spawn(command, args, options), trackOptions);
}

export function cleanupScratch(directory, { retries = 3, retryDelayMs = 100 } = {}) {
  try {
    rmSync(directory, {
      recursive: true,
      force: true,
      maxRetries: retries,
      retryDelay: retryDelayMs,
    });
    return true;
  } catch {
    return false;
  }
}
