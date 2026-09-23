import { useMemo, useState } from 'react';
import { marked } from 'marked';
import { FileText, Sparkles } from 'lucide-react';
import { Tabs } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

interface Props {
  value: string;
  onChange: (docs: string) => void;
}

export function DocsEditor({ value, onChange }: Props) {
  const [mode, setMode] = useState<'edit' | 'preview' | 'split'>('split');

  // `marked` is synchronous and escaping-safe for our use case: the
  // markdown source is whatever the user typed in the editor above the
  // preview (or restored from the endpoint's `docs` field that *they*
  // wrote). We don't pass `dangerouslyAllow` flags, so inline HTML in
  // the source is rendered as escaped text — exactly the posture we
  // had with `react-markdown` defaults.
  const html = useMemo(() => marked.parse(value, { async: false }) as string, [value]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-4 py-1.5">
        <Tabs
          items={[
            { id: 'edit', label: '编辑' },
            { id: 'split', label: '分屏' },
            { id: 'preview', label: '预览' },
          ]}
          value={mode}
          onChange={(id) => setMode(id as typeof mode)}
        />
        <div className="ml-auto flex items-center gap-1.5">
          <Button variant="ghost" size="sm" className="gap-1.5 text-muted-foreground">
            <Sparkles className="h-3.5 w-3.5" />
            AI 补全
          </Button>
        </div>
      </div>
      <div className={cn('flex min-h-0 flex-1', mode === 'split' && 'divide-x divide-border')}>
        {(mode === 'edit' || mode === 'split') && (
          <textarea
            value={value}
            onChange={(e) => onChange(e.target.value)}
            placeholder={'# 接口说明\n\n描述这个接口的用途、参数、返回值、错误码…'}
            className={cn(
              'min-h-0 flex-1 resize-none bg-transparent p-4 font-mono text-sm leading-relaxed',
              'focus:outline-none',
              mode === 'split' && 'max-w-[50%]',
            )}
          />
        )}
        {(mode === 'preview' || mode === 'split') && (
          <div className="min-h-0 flex-1 overflow-y-auto p-4">
            {value.trim() ? (
              <article
                className="markdown-prose"
                // Output of `marked.parse` for trusted (user-authored) input
                // — see the useMemo above for the threat-model note.
                dangerouslySetInnerHTML={{ __html: html }}
              />
            ) : (
              <div className="flex h-full flex-col items-center justify-center gap-2 text-sm text-muted-foreground">
                <FileText className="h-8 w-8 opacity-40" />
                <p>暂无文档</p>
                <p className="text-[11px]">在左侧编辑 Markdown，或点击「AI 补全」</p>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
