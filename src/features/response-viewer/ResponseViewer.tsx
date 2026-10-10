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
import { gsap, useGSAP } from '@/lib/gsap';
import { ResponseHistory } from './ResponseHistory';

export function ResponseViewer() {
  const response = useAppStore((s) => s.lastResponse);
  const pending = useAppStore((s) => s.requestPending);
  // The resource form, not the row form: a response can belong to an
  // endpoint that was never saved, and "保存为示例" keys off this id —
  // which works just as well for a `draft:`-prefixed one.
  const endpoint = useActiveEndpointResource()?.node ?? null;
  const addResponseExample = useAppStore((s) => s.addResponseExample);
  const [savedFlash, setSavedFlash] = useState(false);

  const [view, setView] = useState<'pretty' | 'raw' | 'headers'>('pretty');
  // Pretty tab opens in formatted mode by default — that's the
  // whole point of the tab. When a new response arrives we reset
  // the toggle so a refresh / retry also re-renders pretty JSON
  // (the user can still flip to "原文" mid-inspection).
  const [formatted, setFormatted] = useState<boolean>(true);

  useEffect(() => {
    if (response) setFormatted(true);
  }, [response]);

  const formatResult = useMemo(() => tryFormatJson(response?.body ?? ''), [response?.body]);
  const canFormat = formatResult.ok;

  // C1: status pill pulse + count-up wiring.
  // `statusPillRef` targets the icon + status code + statusText row so
  // the whole pill morphs in as one unit. The two mutable count-up
  // objects live in refs (not state) so gsap can mutate `.val` on
  // every frame without re-rendering for the value alone; we push
  // rounded snapshots into `displayDuration` / `displaySize` state for
  // React to render. Initial `.val` is 0 so the *first* response
  // counts up from 0 → target; for subsequent responses the previous
  // value is preserved across renders and the next animation glides
  // from there to the new target — visible "ramp" only on the very
  // first frame after the pill mounts.
  const statusPillRef = useRef<HTMLDivElement>(null);
  const durObjRef = useRef({ val: 0 });
  const sizeObjRef = useRef({ val: 0 });
  const [displayDuration, setDisplayDuration] = useState(0);
  const [displaySize, setDisplaySize] = useState(0);

  useGSAP(() => {
    // PendingState hides the pill, so bail until the full response UI
    // is mounted. `pending` is in the dep array so we re-fire when
    // pending flips false (a response that arrived *during* a request
    // hasn't been animated yet — handleSend sets lastResponse first,
    // then clears pending, and the second transition is what surfaces
    // the animation).
    if (pending || !response || !statusPillRef.current) return;

    // Pulse the status pill.
    gsap.fromTo(
      statusPillRef.current,
      { scale: 0.92, opacity: 0 },
      { scale: 1, opacity: 1, duration: 0.28, ease: 'power2.out' },
    );

    // Count-up duration (kill any in-flight tween first so back-to-back
    // responses don't pile up overlapping tweens on the same object).
    gsap.killTweensOf(durObjRef.current);
    gsap.to(durObjRef.current, {
      val: response.durationMs,
      duration: 0.4,
      ease: 'power2.out',
      onUpdate: () => setDisplayDuration(Math.round(durObjRef.current.val)),
    });

    // Count-up size.
    gsap.killTweensOf(sizeObjRef.current);
    gsap.to(sizeObjRef.current, {
      val: response.sizeBytes,
      duration: 0.4,
      ease: 'power2.out',
      onUpdate: () => setDisplaySize(Math.round(sizeObjRef.current.val)),
    });
  }, [response?.status, response?.durationMs, response?.sizeBytes, pending]);

  const handleSaveAsExample = () => {
    if (!response || !endpoint) return;
    addResponseExample(endpoint.id, response);
    setSavedFlash(true);
    window.setTimeout(() => setSavedFlash(false), 1500);
  };

  // C3: Save-as-example bounce. Fires only when savedFlash transitions
  // false → true (the next state change to false intentionally no-ops,
  // because the tooltip text has already swapped to "已保存" by then).
  const saveExampleBtnRef = useRef<HTMLButtonElement>(null);
  useGSAP(() => {
    if (!savedFlash || !saveExampleBtnRef.current) return;
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
  }, [savedFlash]);

  if (pending) {
    return <PendingState />;
  }

  if (!response) {
    return (
      <>
        <EmptyState />
        <ResponseHistory />
      </>
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

      {/* Inline error banner — only renders for aborted/timeout/network
          responses (i.e. `status === 0`). Real HTTP error statuses
          (4xx/5xx) still flow through the normal status pill above. */}
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
          items={[
            { id: 'pretty', label: 'Pretty' },
            { id: 'raw', label: 'Raw' },
            {
              id: 'headers',
              label: 'Headers',
              badge: (
                <span className="rounded bg-muted px-1 text-[10px]">
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
    <section className="flex h-full min-h-0 flex-col items-center justify-center border-t border-border bg-card/30 px-4 text-center">
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
