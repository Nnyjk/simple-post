import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

/**
 * InlineError —— 表单、面板、对话框里就地报一条错的那一行。全仓只有这一份行内错误框。
 *
 * 淡底 `bg-destructive/10` 配 `text-destructive`：`text-destructive-foreground` 只配实心填充
 * `bg-destructive`，只写其中一笔会在浅色与深色主题下都读不出「出错」。
 * 密度（padding、字号）与强度由调用点 `className` 给，行高字号各随所在版面。
 */
export function InlineError({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <p
      className={cn(
        'rounded-md border border-destructive/40 bg-destructive/10 text-destructive',
        className,
      )}
    >
      {children}
    </p>
  );
}
