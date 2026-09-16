import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Check, Copy } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

type CopyState = 'idle' | 'success' | 'error';

const RESET_DELAY_MS = 2000;

export interface CopyConfigTileProps {
  icon: ReactNode;
  name: string;
  description: string;
  configJson: string;
}

/**
 * One tile in the "接入配置复制" row. Shows the AI client name, a one-line
 * description, the JSON snippet to copy, and a full-width copy button. The
 * button swaps to a success / error state for `RESET_DELAY_MS`, tracked in a
 * ref so unmount during the window won't leak.
 */
export function CopyConfigTile({
  icon,
  name,
  description,
  configJson,
}: CopyConfigTileProps) {
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
        // Fallback for environments without async clipboard (older Tauri webviews).
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

  return (
    <div className="flex flex-col rounded-md border border-border bg-card/30 p-3">
      <div className="flex items-center gap-2">
        <span className="text-muted-foreground">{icon}</span>
        <span className="text-xs font-semibold text-foreground">{name}</span>
      </div>
      <p className="mt-1 text-[11px] text-muted-foreground">{description}</p>
      <pre className="mt-2 max-h-32 overflow-auto rounded bg-muted/50 p-2 font-mono text-[10px] leading-relaxed text-foreground/90">
{configJson}
      </pre>
      <Button
        variant={copyState === 'success' ? 'secondary' : 'outline'}
        size="sm"
        onClick={handleCopy}
        className={cn('mt-2 w-full gap-1.5', copyState === 'error' && 'border-destructive text-destructive')}
      >
        {copyState === 'success' ? (
          <>
            <Check className="h-3.5 w-3.5" />
            已复制
          </>
        ) : copyState === 'error' ? (
          <>复制失败</>
        ) : (
          <>
            <Copy className="h-3.5 w-3.5" />
            复制配置
          </>
        )}
      </Button>
    </div>
  );
}
