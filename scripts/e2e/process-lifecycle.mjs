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
   * Release the pipes and the process handle once the child is done.
   *
   * Without this, Node's Windows teardown aborts with
   *   `Assertion failed: !(handle->flags & UV_HANDLE_CLOSING)
   *    (src\win\async.c:76)` — exit code 0xC0000409 — *after* every
   * assertion has already printed "all assertions passed". The child
   * process objects stay reachable through the stream handles, so libuv
   * tries to close a handle it already closed on the way out. `process.exit`
   * does not avoid it.
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
