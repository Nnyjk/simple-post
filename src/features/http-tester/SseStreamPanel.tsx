import { useEffect, useRef, useState } from 'react';
import { ArrowDown, Square } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { useAppStore } from '@/stores/app-store';
import { formatBytes, formatDuration } from '@/lib/utils';
import { stopSse } from './stream/stream-sessions';
import type { SseRunState } from '@/stores/slices/stream-slice';

/** 结束原因给用户看的句子。eof 是服务端关掉了连接，不是错误。 */
const END_REASON_LABEL: Record<NonNullable<SseRunState['endReason']>, string> = {
  eof: '服务端关闭了连接',
  stopped: '已停止',
  timeout: '超时',
};

/** 距离底部小于这个像素数就算「跟着最新」。 */
const FOLLOW_THRESHOLD_PX = 24;

/**
 * SSE 响应区：这次流的连接结果、事件列表与停止按钮。
 *
 * 事件从 store 读（按标签索引），故切到别的标签再回来，已经收到的事件还在。列表自动跟随
 * 最新一条，但用户往上滚就停下 —— 正在翻前面的事件时被拽回底部，等于什么也读不完。
 *
 * 面板写明 SSE 不跑请求脚本：请求脚本是 HTTP 请求/响应模型的一部分，流式响应没有「响应
 * 完成后」这个时刻可挂。
 */
export function SseStreamPanel() {
  const tabId = useAppStore((s) => s.activeTabId);
  const run = useAppStore(
    (s) => (s.activeTabId ? s.lastStreams[s.activeTabId]?.sse : undefined) ?? null,
  );
  const [following, setFollowing] = useState(true);
  const scrollRef = useRef<HTMLDivElement>(null);

  const eventCount = run?.events.length ?? 0;

  // 新事件到达时跟随底部；用户上滚后 `following` 为假，这里就不动他的滚动位置。
  useEffect(() => {
    if (!following) return;
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [eventCount, following]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    setFollowing(distance <= FOLLOW_THRESHOLD_PX);
  };

  const running = run !== null && (run.phase === 'connecting' || run.phase === 'streaming');

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border bg-card/30 px-4 py-2 text-xs">
        <Badge variant={run?.phase === 'error' ? 'destructive' : running ? 'default' : 'muted'}>
          {run === null
            ? '未开始'
            : run.phase === 'connecting'
              ? '连接中'
              : run.phase === 'streaming'
                ? '接收中'
                : run.phase === 'ended'
                  ? '已结束'
                  : '失败'}
        </Badge>
        {run?.meta && (
          <>
            <span className="font-mono">
              {run.meta.status} {run.meta.statusText}
            </span>
            <span className="text-muted-foreground">{run.meta.contentType}</span>
          </>
        )}
        {run && (
          <>
            <span className="text-muted-foreground">
              耗时 {formatDuration(run.durationMs ?? Date.now() - run.startedAt)}
            </span>
            <span className="text-muted-foreground">事件 {run.eventCount}</span>
            <span className="text-muted-foreground">{formatBytes(run.bytes)}</span>
          </>
        )}
        {run?.endReason && (
          <span className="text-muted-foreground">{END_REASON_LABEL[run.endReason]}</span>
        )}
        {run?.truncated && <Badge variant="warning">已截断</Badge>}
        {run?.error && (
          <span className="text-destructive">
            {run.error.kind === 'timeout'
              ? '超时'
              : run.error.kind === 'network'
                ? '网络错误'
                : '协议错误'}
            ：{run.error.message}
          </span>
        )}
        <span className="ml-auto text-muted-foreground">SSE 不跑请求脚本</span>
        <Button
          variant="outline"
          size="sm"
          className="h-7 gap-1 px-2"
          disabled={!running || !tabId}
          onClick={() => {
            if (tabId) stopSse(tabId);
          }}
          data-testid="sse-stop-btn"
        >
          <Square className="h-3 w-3" />
          停止
        </Button>
      </div>

      <div className="relative min-h-0 flex-1">
        <div ref={scrollRef} onScroll={onScroll} className="h-full overflow-y-auto px-4 py-3">
          {run === null || run.events.length === 0 ? (
            <p className="text-xs text-muted-foreground">
              {run === null ? '按 Send 建立事件流。' : '等待事件…'}
            </p>
          ) : (
            <ol className="flex flex-col gap-2" data-testid="sse-events">
              {run.eventCount > run.events.length && (
                <li className="text-xs text-muted-foreground">
                  只显示最近 {run.events.length} 条，共 {run.eventCount} 条。
                </li>
              )}
              {run.events.map((event) => (
                <li key={event.seq} className="rounded-md border border-border bg-card/40">
                  <div className="flex flex-wrap items-center gap-2 border-b border-border/60 px-2 py-1 text-micro text-muted-foreground">
                    <span className="font-mono">#{event.seq}</span>
                    <span className="font-mono text-foreground">{event.event}</span>
                    {event.id !== '' && <span className="font-mono">id: {event.id}</span>}
                  </div>
                  <pre className="overflow-x-auto whitespace-pre-wrap break-all px-2 py-1.5 font-mono text-xs">
                    {event.data}
                  </pre>
                </li>
              ))}
            </ol>
          )}
        </div>
        {!following && run !== null && run.events.length > 0 && (
          <Button
            variant="outline"
            size="sm"
            className="absolute bottom-3 right-4 h-7 gap-1 px-2 shadow"
            onClick={() => setFollowing(true)}
            data-testid="sse-follow-latest"
          >
            <ArrowDown className="h-3 w-3" />
            回到最新
          </Button>
        )}
      </div>
    </div>
  );
}
