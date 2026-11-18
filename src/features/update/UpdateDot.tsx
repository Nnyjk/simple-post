/** 有新版本时的红点。三处入口共用一份样式，避免各自写一遍尺寸与描边。 */
import { cn } from '@/lib/utils';

export function UpdateDot({ className }: { className?: string }) {
  return (
    <span
      aria-hidden
      className={cn(
        'absolute -right-0.5 -top-0.5 h-1.5 w-1.5 rounded-full bg-destructive ring-2 ring-background',
        className,
      )}
    />
  );
}
