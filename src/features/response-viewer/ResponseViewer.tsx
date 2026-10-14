import { useEffect, useMemo, useRef, useState } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import { oneDark } from '@codemirror/theme-one-dark';
import {
  Copy,
  Download,
  Clock,
  HardDrive,
  CheckCircle2,
  XCircle,
  BookmarkPlus,
  Check,
  Wand2,
} from 'lucide-react';
import { useAppStore, useActiveEndpointResource } from '@/stores/app-store';
import { Tabs } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { formatBytes, formatDuration, statusColorVar, cn, tryFormatJson } from '@/lib/utils';
import { useIsDark } from '@/lib/theme';
import { lightCodeTheme } from '@/lib/codemirror-themes';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';
import { ResponseHistory } from './ResponseHistory';

export function ResponseViewer() {
  const response = useAppStore((s) => s.lastResponse);
  const pending = useAppStore((s) => s.requestPending);
  // 取资源形式而非行形式：响应可能属于从未保存过的接口，而「保存为示例」以此 id 为键，
  // 对 `draft:` 前缀的 id 同样成立。
  const endpoint = useActiveEndpointResource()?.node ?? null;
  const addResponseExample = useAppStore((s) => s.addResponseExample);
  const [savedFlash, setSavedFlash] = useState(false);

  const [view, setView] = useState<'pretty' | 'raw' | 'headers'>('pretty');
  // Pretty 页默认以格式化形态打开 —— 这正是该页的意义。新响应到达时重置开关，使刷新 /
  // 重试也重新渲染格式化 JSON（用户检查中途仍可翻到「原文」）。
  const [formatted, setFormatted] = useState<boolean>(true);

  useEffect(() => {
    if (response) setFormatted(true);
  }, [response]);

  const formatResult = useMemo(() => tryFormatJson(response?.body ?? ''), [response?.body]);
  const canFormat = formatResult.ok;

  // C1：状态药丸的脉冲 + 数字滚动。
  // `statusPillRef` 指向「图标 + 状态码 + statusText」整行，使整个药丸作为一个整体形变。
  // 两个可变计数对象放在 ref（而非 state）里，gsap 才能逐帧改 `.val` 而不为此数值单独
  // 重渲染；再把取整后的快照推进 `displayDuration` / `displaySize` state 交给 React 渲染。
  // 初始 `.val` 为 0，故第一个响应从 0 滚到目标；此后的响应保留上一次的取值，下一段动画
  // 从那里滑向新目标 —— 可见的「爬升」只出现在药丸挂载后的第一帧。
  const statusPillRef = useRef<HTMLDivElement>(null);
  const durObjRef = useRef({ val: 0 });
  const sizeObjRef = useRef({ val: 0 });
  const [displayDuration, setDisplayDuration] = useState(0);
  const [displaySize, setDisplaySize] = useState(0);

  useGSAP(() => {
    // PendingState 会隐藏药丸，故等完整响应 UI 挂载后再动。`pending` 在依赖数组里，是为了
    // pending 翻回 false 时重跑（请求**进行中**到达的响应还没动画过 —— handleSend 先写
    // lastResponse 再清 pending，第二次跃迁才是动画浮现的那一次）。
    if (pending || !response || !statusPillRef.current) return;

    // 杀掉在途 tween 是状态卫生而非动效本身：不做的话连续两个响应会把重叠的 tween 堆到
    // 同一个对象上，数字跟自己打架。此句留在守卫之外。
    gsap.killTweensOf(durObjRef.current);
    gsap.killTweensOf(sizeObjRef.current);

    return motionSafe(
      () => {
        // Pulse the status pill.
        gsap.fromTo(
          statusPillRef.current,
          { scale: 0.92, opacity: 0 },
          { scale: 1, opacity: 1, duration: 0.28, ease: 'power2.out' },
        );

        // Count-up duration.
        gsap.to(durObjRef.current, {
          val: response.durationMs,
          duration: 0.4,
          ease: 'power2.out',
          onUpdate: () => setDisplayDuration(Math.round(durObjRef.current.val)),
        });

        // Count-up size.
        gsap.to(sizeObjRef.current, {
          val: response.sizeBytes,
          duration: 0.4,
          ease: 'power2.out',
          onUpdate: () => setDisplaySize(Math.round(sizeObjRef.current.val)),
        });
      },
      // 数字滚动不是装饰。`displayDuration` / `displaySize` **只**由 `onUpdate` 写入，跳过
      // tween 会让响应面板对每个请求都显示 `0 ms` / `0 B` —— 降低动效就会静默地让用户
      // 失去测量值。此处直接落终值而非补间过去。
      () => {
        durObjRef.current.val = response.durationMs;
        sizeObjRef.current.val = response.sizeBytes;
        setDisplayDuration(response.durationMs);
        setDisplaySize(response.sizeBytes);
      },
    );
  }, [response?.status, response?.durationMs, response?.sizeBytes, pending]);

  const handleSaveAsExample = () => {
    if (!response || !endpoint) return;
    addResponseExample(endpoint.id, response);
    setSavedFlash(true);
    window.setTimeout(() => setSavedFlash(false), 1500);
  };

  // C3：保存为示例的弹跳。仅在 savedFlash 由 false → true 时触发；下一次变回 false 刻意
  // 为空操作 —— 那时 tooltip 文案早已换成「已保存」。
  const saveExampleBtnRef = useRef<HTMLButtonElement>(null);
  useGSAP(() => {
    if (!savedFlash || !saveExampleBtnRef.current) return;
    return motionSafe(() => {
      gsap.killTweensOf(saveExampleBtnRef.current);
      gsap.fromTo(
        saveExampleBtnRef.current,
        { scale: 1 },
        {
          keyframes: [
            { scale: 1.06, duration: 0.1, ease: 'power2.out' },
            { scale: 1, duration: 0.1, ease: 'power2.in' },
          ],
        },
      );
    });
  }, [savedFlash]);

  if (pending) {
    return <PendingState />;
  }

  if (!response) {
    // 历史条也在这里渲染，故本分支必须与下方有数据分支一样是 **flex 纵向**容器。
    //
    // `WorkspaceShell` 把 viewer 包在 `min-h-0 flex-1` 里，而那本身不是 flex 容器，
    // 故 fragment 的子元素走普通块级流：`h-full` 的 `<section>` 占满整个盒子，紧随其后的
    // `shrink-0` 历史条就落到下边缘之外 —— 不崩溃、不打日志，条在 DOM 里、完整渲染、只是
    // 跑到屏幕外，DOM 查询照样查得到。这类问题单元测试抓不到（本 runner 无 DOM），故
    // `__tests__/response-viewer-layout.test.ts` 在源码层面钉住容器形状。
    return (
      <section className="flex h-full min-h-0 flex-col">
        <EmptyState />
        <ResponseHistory />
      </section>
    );
  }

  const statusColor = statusColorVar(response.status);
  const isError = response.status >= 400;

  return (
    <section className="flex h-full min-h-0 flex-col border-t border-border bg-card/30">
      {/* Status bar */}
      <div className="flex shrink-0 items-center gap-3 border-b border-border px-4 py-2">
        <div ref={statusPillRef} className="flex items-center gap-2 will-change-transform">
          {isError ? (
            <XCircle className="h-4 w-4" style={{ color: statusColor }} />
          ) : (
            <CheckCircle2 className="h-4 w-4" style={{ color: statusColor }} />
          )}
          <span className="text-sm font-semibold" style={{ color: statusColor }}>
            {response.status}
          </span>
          <span className="text-sm text-muted-foreground">{response.statusText}</span>
        </div>
        <div className="flex items-center gap-1 text-xs text-muted-foreground">
          <Clock className="h-3 w-3" />
          {formatDuration(displayDuration)}
        </div>
        <div className="flex items-center gap-1 text-xs text-muted-foreground">
          <HardDrive className="h-3 w-3" />
          {formatBytes(displaySize)}
        </div>
        <div className="ml-auto flex items-center gap-1">
          <Tooltip content={savedFlash ? '已保存' : '另存为示例'} side="bottom">
            <Button
              ref={saveExampleBtnRef}
              variant="ghost"
              size="icon"
              className={cn('h-7 w-7', savedFlash && 'text-success')}
              onClick={handleSaveAsExample}
              disabled={!endpoint}
              data-testid="save-example-btn"
            >
              {savedFlash ? (
                <Check className="h-3.5 w-3.5" />
              ) : (
                <BookmarkPlus className="h-3.5 w-3.5" />
              )}
            </Button>
          </Tooltip>
          <Tooltip content="复制响应体" side="bottom">
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7"
              onClick={() => navigator.clipboard?.writeText(response.body)}
            >
              <Copy className="h-3.5 w-3.5" />
            </Button>
          </Tooltip>
          <Tooltip content="下载响应" side="bottom">
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7"
              onClick={() => {
                const blob = new Blob([response.body], { type: 'text/plain' });
                const url = URL.createObjectURL(blob);
                const a = document.createElement('a');
                a.href = url;
                a.download = 'response.txt';
                a.click();
                URL.revokeObjectURL(url);
              }}
            >
              <Download className="h-3.5 w-3.5" />
            </Button>
          </Tooltip>
        </div>
      </div>

      {/* 行内错误横幅 —— 只对中止 / 超时 / 网络错误（即 `status === 0`）渲染。真正的
          HTTP 错误状态（4xx/5xx）仍走上方正常的状态药丸。 */}
      {response.status === 0 && response.error && (
        <div className="flex shrink-0 items-start gap-2 border-b border-destructive/30 bg-destructive/10 px-4 py-2 text-xs text-destructive">
          <XCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <div className="min-w-0 flex-1">
            <div className="font-medium leading-tight">{response.statusText}</div>
            <div className="mt-0.5 leading-snug opacity-80 break-all">{response.error}</div>
          </div>
        </div>
      )}

      {/* Tabs */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-4 py-1.5">
        <Tabs
          variant="segmented"
          items={[
            { id: 'pretty', label: 'Pretty' },
            { id: 'raw', label: 'Raw' },
            {
              id: 'headers',
              label: 'Headers',
              // 填充徽标在这里行不通：它所在的分段要么是 `--muted` 的槽，要么是 `--chip`
              // 的滑块，没有一种填充能同时与两者拉开对比。计数本就是「有但不需操作」的
              // 信息，故用第三级文字、放弃 chip。
              badge: (
                <span className="text-faint tabular-nums">
                  {Object.keys(response.headers).length}
                </span>
              ),
            },
          ]}
          value={view}
          onChange={(id) => setView(id as typeof view)}
        />
        <div className="ml-auto" />
        {view === 'pretty' && (
          <Tooltip content={canFormat ? undefined : '响应不是合法 JSON'} side="bottom">
            <Button
              variant="outline"
              size="sm"
              className="h-7"
              onClick={() => setFormatted((v) => !v)}
              disabled={!canFormat}
              data-testid="format-response-btn"
            >
              <Wand2 className="h-3.5 w-3.5" />
              {formatted ? '原文' : '格式化'}
            </Button>
          </Tooltip>
        )}
      </div>

      {/* Body */}
      <div className="min-h-0 flex-1 overflow-auto">
        {view === 'pretty' && (
          <PrettyView body={response.body} formatted={formatted} formatResult={formatResult} />
        )}
        {view === 'raw' && (
          <pre className="m-0 whitespace-pre-wrap break-all p-4 font-mono text-xs leading-relaxed text-foreground/90">
            {response.body}
          </pre>
        )}
        {view === 'headers' && <HeadersView headers={response.headers} />}
      </div>

      {/* History (collapsible) */}
      <ResponseHistory />
    </section>
  );
}

function PrettyView({
  body,
  formatted,
  formatResult,
}: {
  body: string;
  formatted: boolean;
  formatResult: ReturnType<typeof tryFormatJson>;
}) {
  const isJson = formatResult.ok;
  const isDark = useIsDark();

  const displayed = useMemo(() => {
    if (formatted && formatResult.ok) return formatResult.formatted;
    return body;
  }, [formatted, formatResult, body]);

  const showInvalidNotice = formatted && !isJson;

  if (!isJson) {
    return (
      <div className="flex h-full flex-col">
        {showInvalidNotice && (
          <div className="alert-warning-inline">响应不是合法 JSON，仅展示原文</div>
        )}
        <pre className="m-0 min-h-0 flex-1 whitespace-pre-wrap break-all p-4 font-mono text-xs leading-relaxed text-foreground/90">
          {body}
        </pre>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      {showInvalidNotice && (
        <div className="alert-warning-inline">响应不是合法 JSON，仅展示原文</div>
      )}
      <div className="min-h-0 flex-1">
        <CodeMirror
          value={displayed}
          extensions={[json()]}
          theme={isDark ? oneDark : lightCodeTheme}
          editable={false}
          basicSetup={{
            lineNumbers: true,
            foldGutter: true,
            highlightActiveLine: false,
          }}
          height="100%"
          className="text-xs"
        />
      </div>
    </div>
  );
}

function HeadersView({ headers }: { headers: Record<string, string> }) {
  return (
    <div className="p-4">
      <table className="w-full text-sm">
        <tbody>
          {Object.entries(headers).map(([k, v]) => (
            <tr key={k} className="border-b border-border/50 last:border-0">
              <td className="w-1/3 py-1.5 pr-4 align-top font-mono text-xs text-muted-foreground">
                {k}
              </td>
              <td className="py-1.5 align-top font-mono text-xs text-foreground/90 break-all">
                {v}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function EmptyState() {
  return (
    // 用 `flex-1` 而非 `h-full`：本 section 在 flex 纵向容器里与历史条平级，`h-full` 会
    // 占满整个盒子把历史条挤出视野；`flex-1` 取的是**剩余**空间 —— 在下方有历史条时，
    // 「填满响应面板」真正的意思就是这个。
    <section className="flex min-h-0 flex-1 flex-col items-center justify-center border-t border-border bg-card/30 px-4 text-center">
      <div className="flex h-12 w-12 items-center justify-center rounded-full bg-muted/40">
        <CheckCircle2 className="h-6 w-6 text-muted-foreground/50" />
      </div>
      <p className="mt-3 text-sm text-muted-foreground">点击「Send」发起请求</p>
      <p className="mt-1 text-[11px] text-muted-foreground/70">响应会显示在这里</p>
    </section>
  );
}

function PendingState() {
  return (
    <section className="flex h-full min-h-0 flex-col items-center justify-center border-t border-border bg-card/30">
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <span className="h-2 w-2 animate-pulse rounded-full bg-primary" />
        <span>请求中…</span>
      </div>
    </section>
  );
}
