import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Check, Copy } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

type CopyState = 'idle' | 'success' | 'error';

const RESET_DELAY_MS = 2000;

export interface CopyConfigTileProps {
  icon: ReactNode;
  /**
   * 可选的客户端名称。不传则不渲染 —— 只支持一个客户端时这个标题纯属重复，上方的卡片
   *   表头已经说明了正在生成哪种格式。
   */
  name?: string;
  description: string;
  configJson: string;
}

/**
 * 「接入配置复制」一行中的一格。显示 AI 客户端名称、一行描述、要复制的 JSON 片段，
 * 以及一个复制按钮。按钮会在 `RESET_DELAY_MS` 内切换为成功 / 失败态，该计时器存在 ref 里，
 * 故窗口期内卸载不会泄漏。
 */
export function CopyConfigTile({ icon, name, description, configJson }: CopyConfigTileProps) {
  const [copyState, setCopyState] = useState<CopyState>('idle');
  const timerRef = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, []);

  const scheduleReset = () => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
    }
    timerRef.current = window.setTimeout(() => {
      setCopyState('idle');
      timerRef.current = null;
    }, RESET_DELAY_MS);
  };

  const handleCopy = async () => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    try {
      if (navigator?.clipboard?.writeText) {
        await navigator.clipboard.writeText(configJson);
      } else {
        // 没有异步剪贴板时改用临时 textarea（较旧的 Tauri webview）。
        const ta = document.createElement('textarea');
        ta.value = configJson;
        ta.style.position = 'fixed';
        ta.style.opacity = '0';
        document.body.appendChild(ta);
        ta.select();
        document.execCommand('copy');
        document.body.removeChild(ta);
      }
      setCopyState('success');
    } catch {
      setCopyState('error');
    }
    scheduleReset();
  };

  // 这里同样不加边框 / 背景壳：本格渲染在「接入配置」分类卡片内部，带边框会让同一个表面
  // 出现两次（card → tile → <pre>）。剩下唯一的嵌套表面就是那个光秃秃的 `<pre>`，而它装的
  // 正是要展示的代码。
  return (
    <div>
      <div className="flex items-center gap-2">
        <span className="shrink-0 text-muted-foreground">{icon}</span>
        {name && <span className="shrink-0 text-xs font-semibold text-foreground">{name}</span>}
        <p className="min-w-0 flex-1 truncate text-[11px] text-muted-foreground">{description}</p>
        <Button
          variant={copyState === 'success' ? 'secondary' : 'outline'}
          size="xs"
          onClick={handleCopy}
          className={cn(
            'shrink-0 gap-1.5',
            copyState === 'error' && 'border-destructive text-destructive',
          )}
        >
          {copyState === 'success' ? (
            <>
              <Check className="h-3 w-3" />
              已复制
            </>
          ) : copyState === 'error' ? (
            <>复制失败</>
          ) : (
            <>
              <Copy className="h-3 w-3" />
              复制配置
            </>
          )}
        </Button>
      </div>
      <pre className="mt-2 max-h-32 overflow-auto rounded-md bg-muted/50 p-2.5 font-mono text-[10px] leading-relaxed text-foreground/90">
        {configJson}
      </pre>
    </div>
  );
}
