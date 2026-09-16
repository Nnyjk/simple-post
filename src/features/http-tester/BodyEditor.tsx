import { useEffect, useMemo, useState } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import { oneDark } from '@codemirror/theme-one-dark';
import type { BodyMode, RequestBody } from '@/types/domain';
import { Tabs } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
import { AlertTriangle, CheckCircle2, Wand2 } from 'lucide-react';
import { cn } from '@/lib/utils';
import { tryFormatJson } from '@/lib/json-format';
import { useIsDark } from '@/lib/theme';
import { lightCodeTheme } from '@/lib/codemirror-themes';

interface Props {
  value: RequestBody;
  onChange: (body: RequestBody) => void;
}

interface BodyContentProps extends Props {
  formatError?: string | null;
}

const BODY_TABS: { id: BodyMode; label: string }[] = [
  { id: 'none', label: 'none' },
  { id: 'json', label: 'json' },
  { id: 'form', label: 'form' },
  { id: 'raw', label: 'raw' },
];

export function BodyEditor({ value, onChange }: Props) {
  const [formatError, setFormatError] = useState<string | null>(null);

  // Switching mode away from json invalidates any prior format error.
  useEffect(() => {
    if (value.mode !== 'json' && formatError !== null) {
      setFormatError(null);
    }
  }, [value.mode, formatError]);

  const handleFormat = () => {
    const r = tryFormatJson(value.content);
    if (r.ok) {
      onChange({ ...value, content: r.formatted });
      setFormatError(null);
    } else {
      setFormatError(r.error);
      window.setTimeout(() => setFormatError(null), 3000);
    }
  };

  return (
    <div className="px-4 py-3">
      <div className="mb-2 flex items-center gap-2">
        <Tabs
          items={BODY_TABS.map((t) => ({ id: t.id, label: t.label }))}
          value={value.mode}
          onChange={(id) => {
            setFormatError(null);
            onChange({ ...value, mode: id as BodyMode });
          }}
        />
        <span className="text-[10px] text-muted-foreground">
          {value.mode === 'json' && 'Content-Type: application/json'}
          {value.mode === 'form' && 'Content-Type: application/x-www-form-urlencoded'}
          {value.mode === 'raw' && 'Content-Type: text/plain'}
        </span>
        <Button
          variant="outline"
          size="sm"
          className="ml-auto h-7"
          disabled={value.mode !== 'json' || !value.content.trim()}
          onClick={handleFormat}
          data-testid="format-body-btn"
        >
          <Wand2 className="h-3.5 w-3.5" />
          格式化
        </Button>
      </div>

      {value.mode === 'none' ? (
        <div className="flex h-32 items-center justify-center rounded-md border border-dashed border-border text-sm text-muted-foreground">
          此请求无 body
        </div>
      ) : (
        <BodyContent value={value} onChange={onChange} formatError={formatError} />
      )}
    </div>
  );
}

function BodyContent({ value, onChange, formatError }: BodyContentProps) {
  const [touched, setTouched] = useState(false);
  const isDark = useIsDark();

  const { valid, error } = useMemo(() => {
    if (value.mode !== 'json') return { valid: true, error: null as string | null };
    if (!value.content.trim()) return { valid: true, error: null };
    try {
      JSON.parse(value.content);
      return { valid: true, error: null };
    } catch (e) {
      return { valid: false, error: (e as Error).message };
    }
  }, [value.mode, value.content]);

  return (
    <div>
      <div className="overflow-hidden rounded-md border border-border">
        <CodeMirror
          value={value.content}
          onChange={(v) => {
            setTouched(true);
            onChange({ ...value, content: v });
          }}
          extensions={value.mode === 'json' ? [json()] : []}
          theme={isDark ? oneDark : lightCodeTheme}
          basicSetup={{
            lineNumbers: true,
            foldGutter: true,
            highlightActiveLine: true,
            highlightActiveLineGutter: true,
          }}
          height="280px"
          className="text-sm"
        />
      </div>
      {formatError && (
        <div className="mt-2 flex items-start gap-1.5 rounded-md border border-red-500/30 bg-red-500/5 px-2.5 py-1.5 text-xs text-red-400">
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span>格式化失败：{formatError}</span>
        </div>
      )}
      {value.mode === 'json' && touched && value.content.trim() && (
        <div
          className={cn(
            'mt-2 flex items-start gap-1.5 rounded-md border px-2.5 py-1.5 text-xs',
            valid
              ? 'border-emerald-500/20 bg-emerald-500/5 text-emerald-400'
              : 'border-amber-500/20 bg-amber-500/5 text-amber-400',
          )}
        >
          {valid ? (
            <>
              <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              <span>JSON 格式正确</span>
            </>
          ) : (
            <>
              <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              <span className="font-mono">{error}</span>
            </>
          )}
        </div>
      )}
    </div>
  );
}
