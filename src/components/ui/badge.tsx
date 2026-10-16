import type { HTMLAttributes, ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: 'default' | 'outline' | 'muted' | 'success' | 'warning' | 'destructive';
  children: ReactNode;
}

/**
 * `destructive` 用 `text-destructive` 而非 `-foreground` 那一档。`-foreground` 是**实心**
 * 填色上的文字档，两个主题正好相反（亮色纯白 `0 0% 100%` / 暗色近黑 `0 0% 10%`），
 * 所以配 10% 的淡色底时，亮色模式就是白底白字。
 *
 * success / warning 是另一回事：它们的 `-foreground` **本身就是**文字档（亮色 30% /
 * 暗色 75%），基础色只够当描边。见 AGENTS.md 的 semantic token pairing。
 */
export function Badge({ variant = 'default', className, children, ...props }: BadgeProps) {
  const variantClass = {
    default: 'bg-primary/15 text-primary border-primary/20',
    outline: 'bg-transparent text-foreground border-border',
    muted: 'bg-muted text-muted-foreground border-transparent',
    success: 'bg-success/15 text-success-foreground border-success/20',
    warning: 'bg-warning/15 text-warning-foreground border-warning/20',
    destructive: 'bg-destructive/10 text-destructive border-destructive/20',
  }[variant];

  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[10px] font-medium',
        variantClass,
        className,
      )}
      {...props}
    >
      {children}
    </span>
  );
}
