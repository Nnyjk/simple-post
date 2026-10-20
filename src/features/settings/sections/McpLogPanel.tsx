/**
 * daemon 日志区 —— 设置页「MCP 进程」卡片下的可折叠日志视图。
 *
 * 三条约束（都来自 issue #6 的验收标准）：
 *
 *   - **默认折叠**：日志可能含环境变量值等敏感信息，不该一进设置页就铺开。
 *   - **有界**：服务端只留最近 500 行，故这一块永远不会长到撑破布局。
 *   - **重启有分界**：`startedAtMs` 在每次 spawn 时变，它变了就是换过一次 daemon，
 *     旧日志与新日志不会混在一起让人读错。
 *
 * 「清屏」是**纯前端**动作：它清掉当前视图，服务端的环形缓冲不动。故按钮写「清屏」
 * 而不是「清空」——后者会让人以为日志被删了。
 */
import { useEffect, useMemo, useRef, useState } from 'react';
import { ChevronDown, ChevronRight, Copy, Eraser } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { useAppStore } from '@/stores/app-store';

/** 展开时多久拉一次日志。与状态轮询同一个节奏，但**不**并进它。 */
const LOG_POLL_MS = 3000;

export function McpLogPanel() {
  const daemonLogs = useAppStore((s) => s.daemonLogs);
  const refreshDaemonLogs = useAppStore((s) => s.refreshDaemonLogs);

  const [open, setOpen] = useState(false);
  // 清屏后从第几行开始显示。环形缓冲转起来时行会左移，故另记一份「已见到的行数」，
  // 长度变小即说明转过了，这时重新从头显示。
  const [hiddenFrom, setHiddenFrom] = useState<number | null>(null);
  const [copied, setCopied] = useState(false);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  const lines = daemonLogs?.lines ?? [];
  const visible = useMemo(() => {
    if (hiddenFrom === null) return lines;
    // 缓冲转过了：行数变少即说明左移了，这时从头显示。
    if (lines.length < hiddenFrom) return lines;
    return lines.slice(hiddenFrom);
  }, [lines, hiddenFrom]);

  // 展开时才拉：日志可能有几百行，每 3s 拉一次是白花的带宽。
  useEffect(() => {
    if (!open) return;
    void refreshDaemonLogs();
    const timer = window.setInterval(() => {
      void refreshDaemonLogs();
    }, LOG_POLL_MS);
    return () => window.clearInterval(timer);
  }, [open, refreshDaemonLogs]);

  // 自动滚到底部——新行在下面，用户要看的正是最新的一行。
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !open) return;
    el.scrollTop = el.scrollHeight;
  }, [visible, open]);

  async function handleCopy() {
    if (visible.length === 0) return;
    try {
      await navigator.clipboard.writeText(visible.join('\n'));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch (e) {
      console.error('[MCP] copy log failed', e);
    }
  }

  const startedAt = daemonLogs?.startedAtMs ?? null;

  return (
    <div className="border-t border-border/60">
      <div className="flex items-center justify-between gap-2 px-3 py-2">
        <button
          type="button"
          className="flex min-w-0 items-center gap-1.5 text-xs font-medium text-foreground"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open}
        >
          {open ? (
            <ChevronDown className="h-3.5 w-3.5 shrink-0" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5 shrink-0" />
          )}
          日志
          {daemonLogs && !daemonLogs.capturing && (
            <span className="text-[11px] font-normal text-muted-foreground">
              （本 daemon 由外部启动，无日志捕获）
            </span>
          )}
        </button>
        {open && (
          <div className="flex shrink-0 items-center gap-1">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-6 gap-1 px-1.5 text-[11px]"
              onClick={() => {
                void handleCopy();
              }}
              disabled={visible.length === 0}
            >
              <Copy className="h-3 w-3" />
              {copied ? '已复制' : '复制'}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-6 gap-1 px-1.5 text-[11px]"
              onClick={() => setHiddenFrom(lines.length)}
              disabled={visible.length === 0}
              title="清掉当前显示；服务端的环形缓冲不动"
            >
              <Eraser className="h-3 w-3" />
              清屏
            </Button>
          </div>
        )}
      </div>

      {open && (
        <div className="px-3 pb-3">
          {startedAt !== null && (
            <div className="mb-1.5 text-[11px] text-muted-foreground">
              本次启动于 {new Date(startedAt).toLocaleString()}
              {daemonLogs?.truncated && ' · 更早的日志已超出缓冲被丢弃'}
            </div>
          )}
          <div
            ref={scrollRef}
            className={cn(
              'max-h-64 overflow-auto rounded-md border border-border/70 bg-muted/30',
              'p-2 font-mono text-[11px] leading-relaxed text-foreground/80',
            )}
          >
            {visible.length === 0 ? (
              <div className="text-muted-foreground">（暂无日志）</div>
            ) : (
              visible.map((line, i) => (
                <div key={`${i}-${line}`} className="whitespace-pre-wrap break-all">
                  {line}
                </div>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  );
}
