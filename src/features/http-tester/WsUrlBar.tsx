import { useState } from 'react';
import { Plug, Unplug } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { ProtocolPicker } from './ProtocolPicker';
import { VarHintInput, type VarEntry } from './var-hint';
import { useAppStore } from '@/stores/app-store';
import { pathParamsBlocked, pathParamsReady } from './model/path-param-gate';
import { closeWs, startWs } from './stream/stream-sessions';
import type { AuthConfig, RequestProtocol, TlsSettings } from '@/types/domain';
import type { PreparedRequest } from '@/lib/http-build/types';

/**
 * WebSocket 的地址栏：协议、地址、子协议与连接动作。
 *
 * 它占住 HTTP 的 `RequestUrlBar` 那一个槽位（页签条之上），故两种协议的纵向次序相同 ——
 * 页签条的下沿贴的永远是面板内容，而不是一条悬在它和面板之间的工具条。WS 的帧与发送框
 * 住在 `WebSocketPanel`，本组件只管「怎么建连」。
 *
 * 边界：不跑请求脚本；鉴权沿用 HTTP 那条规则（Bearer / Basic / API key 已由
 * `prepareRequest` 折算进 `prepared.headers`，OAuth2 的 token 在握手前取好）。
 */
export function WsUrlBar({
  tabId,
  protocol,
  onProtocolChange,
  url,
  onUrlChange,
  varEntries,
  envName,
  prepared,
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
  /** 变量候选：环境变量与动态变量并进同一份列表。 */
  varEntries: VarEntry[];
  /** 当前环境名，显示在候选浮层的标题上。 */
  envName: string | undefined;
  /** 变量替换之后的地址与 header，点击「连接」时原样交给后端。 */
  prepared: PreparedRequest;
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
  // 组装期被拦下的原因属于这条地址栏，不属于帧面板 —— 它说明的是「没连上」，不是「没发出去」。
  const [connectError, setConnectError] = useState<string | null>(null);

  const phase = view?.phase ?? 'closed';
  const open = phase === 'open';
  const blocked = prepared.missingVars.length > 0;
  const pathBlocked = pathParamsBlocked(prepared);

  const connect = () => {
    setConnectError(null);
    // 组装期才发现的问题（`validateRequest` 看到的是替换前的模板，占位符照例放行）：
    // 带着坏掉的描述去握手只会换回一个与真实原因无关的失败，故与 HTTP 发送一样先拦下。
    const problem = prepared.problems[0];
    if (problem) {
      setConnectError(problem.message);
      return;
    }
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
    setConnectError(null);
    void closeWs(tabId);
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
    <div className="flex shrink-0 flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border bg-card/30 px-4 py-2">
        {/* 协议选择器住在这里而不是面板里：它是端点属性，与 HTTP 那份位于同一个槽位，
            用户切到 websocket 之后仍能从原处切回去。宽度与 URL 栏那份对齐。 */}
        <ProtocolPicker
          value={protocol}
          onChange={onProtocolChange}
          className="w-[7.5rem] shrink-0"
        />
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
            // 缺变量不拦：与 HTTP / SSE 一致照发，未替换的 `{{var}}` 原样交给服务端
            // （地址栏下方有提示）。只有路径参数缺值需要拦 —— 带着 `{id}` 握手只会 404。
            disabled={pathBlocked}
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

      {connectError !== null && (
        <p
          className="shrink-0 border-b border-border bg-destructive/10 px-4 py-1.5 text-xs text-destructive"
          data-testid="ws-connect-error"
        >
          {connectError}
        </p>
      )}

      {blocked && (
        <p className="shrink-0 border-b border-border px-4 py-1.5 text-xs text-warning-foreground">
          有未替换的变量：{prepared.missingVars.join('、')}（仍会按原样连接，与 HTTP 一致）
        </p>
      )}

      {pathBlocked && (
        <p className="flex shrink-0 items-center gap-2 border-b border-border px-4 py-1.5 text-xs text-destructive">
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
    </div>
  );
}
