import type { HTMLAttributes, ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: 'default' | 'outline' | 'muted' | 'success' | 'warning' | 'destructive';
  children: ReactNode;
}

export function Badge({ variant = 'default', className, children, ...props }: BadgeProps) {
  const variantClass = {
    default: 'bg-primary/15 text-primary border-primary/20',
    outline: 'bg-transparent text-foreground border-border',
    muted: 'bg-muted text-muted-foreground border-transparent',
    success: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/20',
    warning: 'bg-amber-500/15 text-amber-400 border-amber-500/20',
    destructive: 'bg-red-500/15 text-red-400 border-red-500/20',
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
