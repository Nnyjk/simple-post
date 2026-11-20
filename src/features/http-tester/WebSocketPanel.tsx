import { useEffect, useRef, useState } from 'react';
import { ArrowDown, ArrowLeftRight, Plus, Send } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { KeyValueEditor, type KeySuggestion } from './KeyValueEditor';
import { type VarEntry } from './var-hint';
import { useAppStore } from '@/stores/app-store';
import { formatBytes } from '@/lib/utils';
import { sendWsFrame, setWsReconnect } from './stream/stream-sessions';
import { WS_MAX_RETRIES } from './stream/ws-reconnect';
import type { KeyValue } from '@/types/domain';

/** 距离底部小于这个像素数就算「跟着最新」，与 SSE 面板同一个口径。 */
const FOLLOW_THRESHOLD_PX = 24;

/**
 * WebSocket 面板：自动重连、Headers、帧收发与关闭。
 *
 * 它占住「响应页」的位置，因为 WS 没有「一次请求对应一次响应」这回事 —— 连接活着，两个
 * 方向的帧随时进出。地址、子协议与连接动作住在 `WsUrlBar`（页签条之上那个槽位），本组件
 * 只说「连上之后发生了什么」。
 *
 * 边界：不跑请求脚本（帧的前后没有请求完成这个时刻）。鉴权与 TLS 的合成规则由 `WsUrlBar`
 * 交给 `startWs`，与 `sendPreparedRequest` 用的是同一套。
 */
export function WebSocketPanel({
  tabId,
  headers,
  onHeadersChange,
  headerSuggestions,
  varEntries,
  envName,
}: {
  tabId: string;
  headers: KeyValue[];
  onHeadersChange: (headers: KeyValue[]) => void;
  headerSuggestions: KeySuggestion[];
  /** 变量候选：环境变量与动态变量并进同一份列表。 */
  varEntries: VarEntry[];
  /** 当前环境名，显示在候选浮层的标题上。 */
  envName: string | undefined;
}) {
  const view = useAppStore((s) => s.lastStreams[tabId]?.ws ?? null);
  const [text, setText] = useState('');
  const [sendError, setSendError] = useState<string | null>(null);
  const [following, setFollowing] = useState(true);
  // 倒计时要自己走：`retryAt` 是个时刻，渲染不会因为时间流逝而重跑。
  const [now, setNow] = useState(Date.now());
  const scrollRef = useRef<HTMLDivElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  // 连接代次：文件读取与发送都是异步的，回调可能在连接（或页签）换掉之后才回来。
  const connection = useRef(0);

  const phase = view?.phase ?? 'closed';
  const open = phase === 'open';
  const frameCount = view?.frames.length ?? 0;
  const retryAt = view?.retryAt ?? null;

  // 连接边界清掉上一条发送失败：它说的是上一次连接（或上一个页签）里的事，留着会顶掉
  // 这次连接的 `view.error` —— 渲染里是 `sendError ?? view.error`。连接存活期间的发送
  // 失败照旧可见，发送动作不改变 `phase`。代次一并推进，让还在路上的文件读取与发送
  // 回调认出自己已经过期，别把上一次连接的错误在清除之后又写回来。
  useEffect(() => {
    connection.current += 1;
    setSendError(null);
  }, [phase, tabId]);

  useEffect(() => {
    if (retryAt === null) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(timer);
  }, [retryAt]);

  // 新帧到达时跟随底部；用户上滚之后不动他的位置。
  useEffect(() => {
    if (!following) return;
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [frameCount, following]);

  const send = async (kind: 'text' | 'binary', data: string, base64?: string) => {
    const gen = connection.current;
    setSendError(null);
    try {
      await sendWsFrame(tabId, kind, data, base64);
    } catch (e) {
      // 连接已经换掉：这条失败属于上一次连接，写回来会遮住这次的 `view.error`。
      if (gen !== connection.current) return;
      setSendError(e instanceof Error ? e.message : String(e));
    }
  };

  const sendFile = (file: File) => {
    const gen = connection.current;
    const reader = new FileReader();
    reader.onload = () => {
      if (gen !== connection.current) return;
      const result = typeof reader.result === 'string' ? reader.result : '';
      const comma = result.indexOf(',');
      if (comma < 0) {
        setSendError('读取文件失败');
        return;
      }
      void send('binary', '', result.slice(comma + 1));
    };
    reader.onerror = () => {
      if (gen !== connection.current) return;
      setSendError('读取文件失败');
    };
    reader.readAsDataURL(file);
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-3 border-b border-border px-3 py-1.5 text-xs text-muted-foreground">
        <label className="flex items-center gap-1.5">
          <Switch
            checked={view?.reconnectEnabled ?? false}
            onCheckedChange={(checked) => setWsReconnect(tabId, checked)}
          />
          自动重连
        </label>
        {retryAt !== null && (
          <span data-testid="ws-retry-countdown">
            第 {view?.retryAttempt ?? 0}/{WS_MAX_RETRIES} 次重试 ·{' '}
            {Math.max(0, Math.ceil((retryAt - now) / 1000))}s 后
          </span>
        )}
        <span className="ml-auto">
          WS 不跑请求脚本；鉴权与 HTTP 同一套规则（OAuth2 在建连前取 token）
        </span>
      </div>

      <div className="flex min-h-0 flex-1 flex-col md:flex-row">
        <div className="min-h-0 shrink-0 overflow-y-auto border-b border-border md:w-80 md:border-b-0 md:border-r">
          <div className="flex items-center gap-1.5 px-3 pt-2 text-xs font-medium text-muted-foreground">
            <ArrowLeftRight className="h-3.5 w-3.5" />
            Headers
          </div>
          <KeyValueEditor
            items={headers}
            onChange={onHeadersChange}
            keyPlaceholder="Header 名"
            valuePlaceholder="值"
            keySuggestions={headerSuggestions}
            varEntries={varEntries}
            envName={envName}
          />
        </div>

        <div className="relative flex min-h-0 flex-1 flex-col">
          <div
            ref={scrollRef}
            onScroll={() => {
              const el = scrollRef.current;
              if (!el) return;
              setFollowing(el.scrollHeight - el.scrollTop - el.clientHeight <= FOLLOW_THRESHOLD_PX);
            }}
            className="min-h-0 flex-1 overflow-y-auto px-3 py-2"
          >
            {frameCount === 0 ? (
              <p className="text-xs text-muted-foreground">
                {open ? '已连接，等待帧…' : '连接之后，收发到的帧按时间顺序列在这里。'}
              </p>
            ) : (
              <ol className="flex flex-col gap-1" data-testid="ws-frames">
                {view !== null && view.frames.length < (view.frameCount ?? frameCount) && (
                  <li className="text-xs text-muted-foreground">
                    只显示最近 {view.frames.length} 帧，共 {view.frameCount} 帧。
                  </li>
                )}
                {view?.frames.map((frame, i) => (
                  <li
                    key={`${frame.at}-${i}`}
                    className="flex items-start gap-2 rounded border border-border/60 px-2 py-1 text-xs"
                  >
                    <span
                      className={
                        frame.direction === 'in'
                          ? 'shrink-0 font-mono text-primary'
                          : 'shrink-0 font-mono text-muted-foreground'
                      }
                      title={frame.direction === 'in' ? '收到' : '发出'}
                    >
                      {frame.direction === 'in' ? '←' : '→'}
                    </span>
                    <span className="shrink-0 font-mono text-nano text-muted-foreground">
                      {new Date(frame.at).toLocaleTimeString()} · {formatBytes(frame.bytes)}
                    </span>
                    {frame.kind === 'text' ? (
                      <pre className="min-w-0 flex-1 whitespace-pre-wrap break-all font-mono">
                        {frame.data}
                      </pre>
                    ) : (
                      <span className="min-w-0 flex-1 break-all font-mono text-muted-foreground">
                        二进制 {formatBytes(frame.bytes)}
                        {frame.base64 ? ` · ${frame.base64}` : ''}
                      </span>
                    )}
                  </li>
                ))}
              </ol>
            )}
          </div>
          {!following && frameCount > 0 && (
            <Button
              variant="outline"
              size="sm"
              className="absolute bottom-2 right-3 h-7 gap-1 px-2 shadow"
              onClick={() => setFollowing(true)}
              data-testid="ws-follow-latest"
            >
              <ArrowDown className="h-3 w-3" />
              回到最新
            </Button>
          )}

          <div className="shrink-0 border-t border-border px-3 py-2">
            <div className="flex items-center gap-2">
              <Input
                value={text}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key !== 'Enter' || e.shiftKey || !open) return;
                  // IME 守卫：组合输入期间（中文拼音等）浏览器照样为 Enter 派发 keydown，
                  // 不跳过就会在候选窗上屏的同时把半个词条发出去。与 `App.tsx` 的全局
                  // 快捷键同一组判断：合成事件的 `isComposing` 挂在 `nativeEvent` 上，
                  // 老浏览器回落到 keyCode 229（`e.isComposing` 在 React 合成事件上没有）。
                  if (e.nativeEvent.isComposing || e.keyCode === 229) return;
                  e.preventDefault();
                  void send('text', text);
                }}
                placeholder={open ? '要发送的文本帧，Enter 发送' : '连接后可发送'}
                disabled={!open}
                className="h-8 font-mono text-sm"
                data-testid="ws-text-input"
              />
              <Button
                size="sm"
                className="h-8 gap-1"
                disabled={!open}
                onClick={() => void send('text', text)}
                data-testid="ws-send-btn"
              >
                <Send className="h-3.5 w-3.5" />
                发送
              </Button>
              <Button
                variant="outline"
                size="sm"
                className="h-8 gap-1"
                disabled={!open}
                onClick={() => fileRef.current?.click()}
              >
                <Plus className="h-3.5 w-3.5" />
                文件
              </Button>
              <input
                ref={fileRef}
                type="file"
                className="hidden"
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  if (file) sendFile(file);
                  e.target.value = '';
                }}
              />
            </div>
            {(sendError !== null || view?.error) && (
              <p className="mt-1.5 text-xs text-destructive" data-testid="ws-error">
                {sendError ??
                  `${view?.error?.kind === 'timeout' ? '超时' : view?.error?.kind === 'protocol' ? '协议错误' : '网络错误'}：${view?.error?.message}`}
              </p>
            )}
            {view !== null && view.closeReason !== '' && (
              <p className="mt-1.5 text-xs text-muted-foreground">关闭原因：{view.closeReason}</p>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
