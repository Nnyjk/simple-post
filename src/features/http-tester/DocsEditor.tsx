import { useState } from 'react';
import ReactMarkdown from 'react-markdown';
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
      <div
        className={cn(
          'flex min-h-0 flex-1',
          mode === 'split' && 'divide-x divide-border',
        )}
      >
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
              <article className="markdown-prose">
                <ReactMarkdown>{value}</ReactMarkdown>
              </article>
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
