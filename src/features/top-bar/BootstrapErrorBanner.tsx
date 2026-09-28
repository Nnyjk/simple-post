/**
 * Startup-load failure banner.
 *
 * `bootstrap()` hydrates projects / modules / collections / endpoints /
 * environments in one `Promise.all`. If any of those five commands
 * fails, the slice keeps the last-known data and records the message in
 * `bootstrapError` — it deliberately does NOT substitute empty arrays,
 * so a backend outage is never rendered as "you have no projects".
 *
 * That distinction only matters if something reads the flag. Until this
 * component existed, `bootstrapError` was write-only: a failed startup
 * looked exactly like an empty database, and the user got an app full of
 * blank panels with no hint that the daemon was down.
 *
 * Two exits, both deliberate:
 *   - 重试 re-runs `bootstrap()`. It early-returns only when `loaded` is
 *     true or a load is in flight, so a failed attempt is retryable.
 *   - 忽略 clears the message without retrying, for the case where the
 *     user understands the failure and wants the empty UI out of the way.
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
      {/* Backend messages are arbitrary strings (JSON-RPC errors, HTTP
          status text); truncate so a long one can't push the buttons
          off-screen. The full text stays in the DOM for copy/inspect. */}
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
