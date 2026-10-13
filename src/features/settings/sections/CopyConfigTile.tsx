import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Check, Copy } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

type CopyState = 'idle' | 'success' | 'error';

const RESET_DELAY_MS = 2000;

export interface CopyConfigTileProps {
  icon: ReactNode;
  /**
   * Optional client name. Renders nothing when omitted — with a single
   * supported client the title is pure redundancy, since the card
   * header above it already says which format is being generated.
   */
  name?: string;
  description: string;
  configJson: string;
}

/**
 * One tile in the "接入配置复制" row. Shows the AI client name, a one-line
 * description, the JSON snippet to copy, and a full-width copy button. The
 * button swaps to a success / error state for `RESET_DELAY_MS`, tracked in a
 * ref so unmount during the window won't leak.
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

  // No border/bg shell here either: the tile renders inside the
  // "接入配置" category card, so a bordered tile made the same surface
  // appear twice (card → tile → <pre>). One plain <pre> is the only
  // nested surface left, and it is the code being shown.
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
