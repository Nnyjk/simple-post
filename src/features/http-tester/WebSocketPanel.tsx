import { useEffect, useRef, useState } from 'react';
import { ArrowDown, ArrowLeftRight, Plug, Plus, Send, Unplug } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { KeyValueEditor, type KeySuggestion } from './KeyValueEditor';
import { ProtocolPicker } from './ProtocolPicker';
import { VarHintInput, type VarEntry } from './var-hint';
import { useAppStore } from '@/stores/app-store';
import { formatBytes } from '@/lib/utils';
import { pathParamsBlocked, pathParamsReady } from './model/path-param-gate';
import { closeWs, sendWsFrame, setWsReconnect, startWs } from './stream/stream-sessions';
import { WS_MAX_RETRIES } from './stream/ws-reconnect';
import type { AuthConfig, KeyValue, RequestProtocol, TlsSettings } from '@/types/domain';
import type { PreparedRequest } from '@/lib/http-build/types';

/** 距离底部小于这个像素数就算「跟着最新」，与 SSE 面板同一个口径。 */
const FOLLOW_THRESHOLD_PX = 24;

/**
 * WebSocket 面板：连接、帧收发、关闭。
 *
 * 它替换请求/响应分栏，因为 WS 没有「一次请求对应一次响应」这回事 —— 连接活着，两个方向
 * 的帧随时进出。URL 与 Headers 仍写草稿，和 HTTP 端点共用同一份字段与同一套变量替换；
 * 变量替换在点击「连接」时取自 `prepared`（`prepareRequest` 的结果），面板不另写一套。
 *
 * 边界：不跑请求脚本（帧的前后没有请求完成这个时刻）。鉴权复用 HTTP 那条规则：Bearer /
 * Basic / API key 已由 `prepareRequest` 折算进 `prepared.headers`（query 档的 key 折进 URL），
 * OAuth2 的 token 在点击「连接」后、握手之前取好并注入 —— 与 `sendPreparedRequest` 用的是同一
 * 对 `fetchOAuth2Token` / `applyOAuth2Token` 与同一份变量池。TLS 与 HTTP 同一套合成规则：
 * 全局设置在前，端点自己那份整体取代它。
 */
export function WebSocketPanel({
  tabId,
  protocol,
  onProtocolChange,
  url,
  onUrlChange,
  headers,
  onHeadersChange,
  headerSuggestions,
  prepared,
  varEntries,
  envName,
  onPathParamsMissing,
  tls,
  tlsOverride,
  auth,
}: {
  tabId: string;
  protocol: RequestProtocol;
  onProtocolChange: (next: RequestProtocol) => void;
  url: string;
  onUrlChange: (value: string) => void;
  headers: KeyValue[];
  onHeadersChange: (headers: KeyValue[]) => void;
  headerSuggestions: KeySuggestion[];
  /** 变量替换之后的地址与 header，点击「连接」时原样交给后端。 */
  prepared: PreparedRequest;
  varEntries: VarEntry[];
  envName: string | undefined;
  /** 路径参数缺值时交给标签切到「路径参数」页；与 HTTP 发送走同一个回调。 */
  onPathParamsMissing: (missing: string[]) => void;
  /** 全局默认的 HTTPS 出站设置；与 HTTP 发送同一份。 */
  tls: TlsSettings;
  /** 端点自己那份，`null` = 继承全局。 */
  tlsOverride: TlsSettings | null;
  /** 端点的鉴权配置：只有 OAuth2 需要在握手前取 token，其余已在 `prepared` 里。 */
  auth: AuthConfig | undefined;
}) {
  const view = useAppStore((s) => s.lastStreams[tabId]?.ws ?? null);
  const [protocolsInput, setProtocolsInput] = useState('');
  const [text, setText] = useState('');
  const [sendError, setSendError] = useState<string | null>(null);
  const [following, setFollowing] = useState(true);
  // 倒计时要自己走：`retryAt` 是个时刻，渲染不会因为时间流逝而重跑。
  const [now, setNow] = useState(Date.now());
  const scrollRef = useRef<HTMLDivElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const phase = view?.phase ?? 'closed';
  const open = phase === 'open';
  const blocked = prepared.missingVars.length > 0;
  const pathBlocked = pathParamsBlocked(prepared);
  const frameCount = view?.frames.length ?? 0;
  const retryAt = view?.retryAt ?? null;

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

  const connect = () => {
    setSendError(null);
    // 与 HTTP 发送、SSE 建连同一道路径参数闸门：带着 `{id}` 去握手，服务端只会回 404。
    if (!pathParamsReady(prepared, onPathParamsMissing)) return;
    void startWs(tabId, {
      request: prepared,
      // 子协议按逗号分隔，与浏览器 WebSocket 构造函数的写法一致。
      protocols: protocolsInput
        .split(',')
        .map((p) => p.trim())
        .filter((p) => p !== ''),
      tls,
      tlsOverride,
      auth,
    });
  };

  const disconnect = () => {
    setSendError(null);
    void closeWs(tabId);
  };

  const send = async (kind: 'text' | 'binary', data: string, base64?: string) => {
    setSendError(null);
    try {
      await sendWsFrame(tabId, kind, data, base64);
    } catch (e) {
      setSendError(e instanceof Error ? e.message : String(e));
    }
  };

  const sendFile = (file: File) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = typeof reader.result === 'string' ? reader.result : '';
      const comma = result.indexOf(',');
      if (comma < 0) {
        setSendError('读取文件失败');
        return;
      }
      void send('binary', '', result.slice(comma + 1));
    };
    reader.onerror = () => setSendError('读取文件失败');
    reader.readAsDataURL(file);
  };

  const statusLabel =
    phase === 'open'
      ? '已连接'
      : phase === 'connecting'
        ? '连接中'
        : phase === 'error'
          ? '连接失败'
          : '未连接';

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-3 py-2">
        <ProtocolPicker value={protocol} onChange={onProtocolChange} />
        <div className="min-w-0 flex-1">
          <VarHintInput
            value={url}
            onChange={onUrlChange}
            entries={varEntries}
            envName={envName}
            placeholder="ws:// 或 wss:// 地址（支持 {{var}}）"
            className="h-8 font-mono text-sm"
            data-testid="ws-url"
          />
        </div>
        <Input
          value={protocolsInput}
          onChange={(e) => setProtocolsInput(e.target.value)}
          placeholder="子协议（逗号分隔，可选）"
          className="h-8 w-56 font-mono text-sm"
          aria-label="WebSocket 子协议"
        />
        {open || phase === 'connecting' ? (
          <Button variant="outline" size="sm" className="h-8 gap-1" onClick={disconnect}>
            <Unplug className="h-3.5 w-3.5" />
            断开
          </Button>
        ) : (
          <Button
            size="sm"
            className="h-8 gap-1"
            onClick={connect}
            disabled={blocked || pathBlocked}
          >
            <Plug className="h-3.5 w-3.5" />
            连接
          </Button>
        )}
        <Badge variant={phase === 'open' ? 'success' : phase === 'error' ? 'destructive' : 'muted'}>
          {statusLabel}
          {view?.closeCode !== null && view?.closeCode !== undefined && ` · ${view.closeCode}`}
        </Badge>
      </div>

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

      {blocked && (
        <p className="shrink-0 border-b border-border px-3 py-1.5 text-xs text-destructive">
          有未替换的变量：{prepared.missingVars.join('、')}
        </p>
      )}

      {pathBlocked && (
        <p className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-1.5 text-xs text-destructive">
          路径参数 {prepared.missingPathParams.join('、')} 没有填值，填好才能连接
          <Button
            variant="outline"
            size="sm"
            className="h-6 px-2 text-xs"
            onClick={() => onPathParamsMissing(prepared.missingPathParams)}
          >
            去填路径参数
          </Button>
        </p>
      )}

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
                  if (e.key === 'Enter' && !e.shiftKey && open) {
                    e.preventDefault();
                    void send('text', text);
                  }
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
