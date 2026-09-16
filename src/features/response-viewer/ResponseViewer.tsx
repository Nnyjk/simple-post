import { useMemo, useState } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import { oneDark } from '@codemirror/theme-one-dark';
import { Copy, Download, AlertTriangle, Clock, HardDrive, CheckCircle2, XCircle, BookmarkPlus, Check, Wand2 } from 'lucide-react';
import { useAppStore, useActiveEndpoint } from '@/stores/app-store';
import { Tabs } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { formatBytes, formatDuration, statusColorVar, cn } from '@/lib/utils';
import { tryFormatJson } from '@/lib/json-format';
import { useIsDark } from '@/lib/theme';
import { lightCodeTheme } from '@/lib/codemirror-themes';
import { ResponseHistory } from './ResponseHistory';

export function ResponseViewer() {
  const response = useAppStore((s) => s.lastResponse);
  const pending = useAppStore((s) => s.requestPending);
  const endpoint = useActiveEndpoint();
  const addResponseExample = useAppStore((s) => s.addResponseExample);
  const [savedFlash, setSavedFlash] = useState(false);

  const [view, setView] = useState<'pretty' | 'raw' | 'headers'>('pretty');
  const [formatted, setFormatted] = useState<boolean>(false);

  const formatResult = useMemo(() => tryFormatJson(response?.body ?? ''), [response?.body]);
  const canFormat = formatResult.ok;

  const handleSaveAsExample = () => {
    if (!response || !endpoint) return;
    addResponseExample(endpoint.id, response);
    setSavedFlash(true);
    window.setTimeout(() => setSavedFlash(false), 1500);
  };

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
        <div className="flex items-center gap-2">
          {isError ? (
            <XCircle className="h-4 w-4" style={{ color: statusColor }} />
          ) : (
            <CheckCircle2 className="h-4 w-4" style={{ color: statusColor }} />
          )}
          <span
            className="text-sm font-semibold"
            style={{ color: statusColor }}
          >
            {response.status}
          </span>
          <span className="text-sm text-muted-foreground">{response.statusText}</span>
        </div>
        <div className="flex items-center gap-1 text-xs text-muted-foreground">
          <Clock className="h-3 w-3" />
          {formatDuration(response.durationMs)}
        </div>
        <div className="flex items-center gap-1 text-xs text-muted-foreground">
          <HardDrive className="h-3 w-3" />
          {formatBytes(response.sizeBytes)}
        </div>
        <div className="ml-auto flex items-center gap-1">
          <Tooltip content={savedFlash ? '已保存' : '另存为示例'} side="bottom">
            <Button
              variant="ghost"
              size="icon"
              className={cn('h-7 w-7', savedFlash && 'text-emerald-400')}
              onClick={handleSaveAsExample}
              disabled={!endpoint}
              data-testid="save-example-btn"
            >
              {savedFlash ? <Check className="h-3.5 w-3.5" /> : <BookmarkPlus className="h-3.5 w-3.5" />}
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

      {/* Tabs */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-4 py-1.5">
        <Tabs
          items={[
            { id: 'pretty', label: 'Pretty' },
            { id: 'raw', label: 'Raw' },
            { id: 'headers', label: 'Headers', badge: <span className="rounded bg-muted px-1 text-[10px]">{Object.keys(response.headers).length}</span> },
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
        {view === 'pretty' && <PrettyView body={response.body} formatted={formatted} />}
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

function PrettyView({ body, formatted }: { body: string; formatted: boolean }) {
  const formatResult = useMemo(() => tryFormatJson(body), [body]);
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
          <div className="shrink-0 border-b border-amber-500/30 bg-amber-500/5 px-2.5 py-1.5 text-xs text-amber-400">
            响应不是合法 JSON，仅展示原文
          </div>
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
        <div className="shrink-0 border-b border-amber-500/30 bg-amber-500/5 px-2.5 py-1.5 text-xs text-amber-400">
          响应不是合法 JSON，仅展示原文
        </div>
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
              <td className="w-1/3 py-1.5 pr-4 align-top font-mono text-xs text-blue-400">
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
      <p className="mt-1 text-[11px] text-muted-foreground/70">
        响应会显示在这里（当前为 mock 响应）
      </p>
    </section>
  );
}

function PendingState() {
  return (
    <section className="flex h-full min-h-0 flex-col items-center justify-center border-t border-border bg-card/30">
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <span className="h-2 w-2 animate-pulse rounded-full bg-blue-400" />
        <span>请求中…</span>
      </div>
    </section>
  );
}
