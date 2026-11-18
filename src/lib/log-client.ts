/**
 * 渲染进程的日志上报口。
 *
 * 三条来源都汇到 Rust 的同一个环形缓冲：未捕获异常、未处理的 rejection、`console.error`。
 * 导出按钮读的是同一份缓冲，故用户导出的文件里既有后端事件也有前端错误。
 *
 * 上报是**fire-and-forget**：它自己绝不抛、从不阻塞调用方。一个上报失败（例如面向
 * `serve-web` 时 bearer 过期）不该把正在复现的那个问题变成第二个异常。
 */
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';

/**
 * 单条上报的字符上限。与 Rust 侧 `CLIENT_FIELD_LIMIT_CHARS` 同值：一次
 * `JSON.stringify` 的整份状态若原样发过去，一条记录就能把 2000 条的缓冲挤掉一半。
 */
const MAX_EVENT_CHARS = 4000;

/** 幂等安装的守卫：`main.tsx` 重跑（HMR）时不重复包装 `console.error`。 */
let installed = false;

/** 上报进行中的递归保护：上报路径内部绝不再调 `console`。 */
let reporting = false;

/**
 * 把一个来源不明的值转成「一行标题 + 堆栈」的文本。
 *
 * `Error` 才有堆栈；其余值（字符串、对象、`Object.create(null)`）走 `String`，故
 * `String` 自己会抛的情况也仍然给出可读文本。
 */
export function describeError(err: unknown): string {
  if (err instanceof Error) {
    return `${err.name}: ${err.message}\n${err.stack ?? ''}`;
  }
  try {
    return String(err);
  } catch {
    return '未知错误';
  }
}

/**
 * 记一条前端事件。级别拼错不失败：Rust 侧按 `Info` 处理。
 *
 * 返回值是 `void` 而非 Promise，因为这些调用点——`window.onerror`、rejection 监听、
 * 包装后的 `console.error`——都无法等待。
 */
export function logClientEvent(
  level: string,
  source: string,
  message: string,
  detail?: string,
): void {
  try {
    // 按码点截断，故多字节字符不会被切坏。
    void invokeCmd<void>(COMMANDS.logClientEvent, {
      level,
      source,
      message: Array.from(message).slice(0, MAX_EVENT_CHARS).join(''),
      detail:
        detail === undefined ? undefined : Array.from(detail).slice(0, MAX_EVENT_CHARS).join(''),
    }).catch(() => {
      // 缓冲区没写进去也不该冒泡：这会变成「日志失败」的第二条错误。
    });
  } catch {
    // 同上。`invokeCmd` 在 web 形态下缺路由时可能同步抛。
  }
}

/**
 * 安装全局错误捕获。幂等：重复调用只生效第一次。
 *
 * 由 `main.tsx` 在 React 挂载之前调用，故初始化期间抛出的错误同样进缓冲区。
 */
export function installGlobalErrorLogging(): void {
  if (installed) return;
  installed = true;

  window.addEventListener('error', (event) => {
    const { message, filename, lineno, colno, error } = event;
    const detail = filename === '' ? undefined : `${filename}:${lineno}:${colno}`;
    logClientEvent(
      'error',
      'window',
      error instanceof Error ? describeError(error) : message,
      detail,
    );
  });

  window.addEventListener('unhandledrejection', (event) => {
    logClientEvent('error', 'promise', describeError(event.reason));
  });

  // 包装原始 `console.error`：先让它照常输出，再抄一份进缓冲区。上报期间置
  // `reporting`，故上报路径内部即使有人调 `console` 也不会再触发一次上报。
  const original = console.error;
  console.error = (...args: unknown[]) => {
    original.apply(console, args);
    if (reporting) return;
    reporting = true;
    try {
      logClientEvent('error', 'console', args.map(describeError).join(' '));
    } finally {
      reporting = false;
    }
  };
}
