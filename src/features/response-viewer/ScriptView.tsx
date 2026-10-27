import { CheckCircle2, XCircle } from 'lucide-react';
import type { ScriptOutcome } from '@/types/domain';

/**
 * 脚本的产出：断言、日志、错误、写回的环境变量。
 *
 * **脚本的成败与请求的成败分开呈现**：断言失败在这里列出，不改写上方状态药丸——
 * 请求成了就是成了，脚本的事是脚本的事。
 */
export function ScriptView({ outcome }: { outcome: ScriptOutcome | null }) {
  if (!outcome) {
    return (
      <div className="flex h-full items-center justify-center p-4 text-sm text-muted-foreground">
        这次请求没有跑脚本
      </div>
    );
  }
  const failed = outcome.assertions.filter((a) => !a.passed);
  return (
    <div className="space-y-4 p-4 text-xs">
      {outcome.errors.length > 0 && (
        <section>
          <h4 className="mb-1.5 font-medium text-destructive">脚本失败</h4>
          <ul className="space-y-1">
            {outcome.errors.map((err, i) => (
              <li
                key={i}
                className="rounded border border-destructive/30 bg-destructive/10 px-2 py-1.5"
              >
                <div className="flex items-center gap-2">
                  <span className="font-mono text-[10px] uppercase text-destructive">
                    {err.kind}
                  </span>
                  <span className="text-muted-foreground">
                    {err.phase === 'pre' ? '前置' : '测试'}阶段
                    {err.source ? ` · ${err.source}` : ''}
                  </span>
                </div>
                <div className="mt-0.5 break-all text-foreground/90">{err.message}</div>
              </li>
            ))}
          </ul>
        </section>
      )}

      {outcome.assertions.length > 0 && (
        <section>
          <h4 className="mb-1.5 font-medium">
            断言
            {failed.length > 0 && (
              <span className="ml-1 text-destructive">{failed.length} 条失败</span>
            )}
          </h4>
          <ul className="space-y-1">
            {outcome.assertions.map((a, i) => (
              <li key={i} className="flex items-start gap-1.5">
                {a.passed ? (
                  <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0 text-success" />
                ) : (
                  <XCircle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-destructive" />
                )}
                <span className="min-w-0">
                  <span className="break-words">{a.name}</span>
                  {a.error && <span className="ml-1 text-destructive">{a.error}</span>}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {outcome.envWrites.length > 0 && (
        <section>
          <h4 className="mb-1.5 font-medium">写回的环境变量</h4>
          <ul className="space-y-0.5 font-mono">
            {outcome.envWrites.map((w, i) => (
              <li key={i} className="break-all">
                <span className="text-muted-foreground">{w.key}</span>
                <span className="mx-1 text-muted-foreground">=</span>
                {w.value === null ? <span className="text-destructive">（已删除）</span> : w.value}
              </li>
            ))}
          </ul>
        </section>
      )}

      {outcome.logs.length > 0 && (
        <section>
          <h4 className="mb-1.5 font-medium">日志</h4>
          <pre className="m-0 whitespace-pre-wrap break-all rounded border border-border/60 bg-muted/20 p-2 font-mono text-[11px] leading-relaxed">
            {outcome.logs.map((l) => `[${l.level}] ${l.message}`).join('\n')}
          </pre>
        </section>
      )}

      {outcome.errors.length === 0 &&
        outcome.assertions.length === 0 &&
        outcome.logs.length === 0 &&
        outcome.envWrites.length === 0 && (
          <div className="text-muted-foreground">脚本跑完了，没有产出。</div>
        )}
    </div>
  );
}
