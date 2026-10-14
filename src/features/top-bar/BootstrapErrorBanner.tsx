/**
 * 启动加载失败横幅。
 *
 * `bootstrap()` 在一个 `Promise.all` 里 hydrate 项目 / 模块 / 集合 / 接口 / 环境。五个命令
 * 中任一失败时，slice 保留最后已知的数据，并把消息记进 `bootstrapError` —— 它刻意**不**用
 * 空数组顶替，故后端故障不会被渲染成「你没有项目」。
 *
 * 这个区分只有在有人读这个标志时才有意义。本组件读它。
 *
 * 两个出口，都是刻意的：
 *   - 重试 重跑 `bootstrap()`。它只在 `loaded` 为真或已有加载在进行时提前返回，故失败的
 *     一次是可重试的。
 *   - 忽略 不重试、直接清掉消息，供用户已理解失败原因、只想让空界面让开的情形。
 */
import { AlertTriangle, Loader2, RotateCw, X } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

export function BootstrapErrorBanner() {
  const bootstrapError = useAppStore((s) => s.bootstrapError);
  const loading = useAppStore((s) => s.loading);
  const bootstrap = useAppStore((s) => s.bootstrap);
  const clearBootstrapError = useAppStore((s) => s.clearBootstrapError);

  if (!bootstrapError) return null;

  return (
    <div
      role="alert"
      aria-live="assertive"
      data-testid="bootstrap-error"
      className="flex shrink-0 items-center gap-2 border-b border-destructive/40 bg-destructive/10 px-3 py-1.5 text-xs text-destructive"
    >
      <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
      <span className="font-medium">启动加载失败</span>
      {/* 后端消息是任意字符串（JSON-RPC 错误、HTTP 状态文本）；截断以免过长把按钮挤出
          屏幕。完整文本仍留在 DOM 里，便于复制 / 查看。 */}
      <span className="min-w-0 flex-1 truncate opacity-90" title={bootstrapError}>
        {bootstrapError}
      </span>
      <Button
        variant="ghost"
        size="sm"
        onClick={() => void bootstrap()}
        disabled={loading}
        className="h-6 gap-1 px-2 text-xs text-destructive hover:bg-destructive/15 hover:text-destructive"
      >
        {loading ? <Loader2 className="h-3 w-3 animate-spin" /> : <RotateCw className="h-3 w-3" />}
        重试
      </Button>
      <Button
        variant="ghost"
        size="icon"
        onClick={clearBootstrapError}
        aria-label="忽略启动错误"
        className={cn('h-6 w-6 text-destructive hover:bg-destructive/15 hover:text-destructive')}
      >
        <X className="h-3.5 w-3.5" />
      </Button>
    </div>
  );
}
