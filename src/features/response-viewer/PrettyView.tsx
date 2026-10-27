import { useMemo } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import { oneDark } from '@codemirror/theme-one-dark';
import { useIsDark } from '@/lib/theme';
import { lightCodeTheme } from '@/lib/codemirror-themes';
import type { FormatResult } from '@/lib/utils';

/**
 * 响应体的格式化视图。
 *
 * `formatted` 为真时展示格式化后的 JSON；响应不是合法 JSON 时退回原文并给出提示。
 */
export function PrettyView({
  body,
  formatted,
  formatResult,
}: {
  body: string;
  formatted: boolean;
  formatResult: FormatResult;
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
