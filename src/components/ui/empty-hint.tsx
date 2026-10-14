import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

/**
 * EmptyHint —— 列表为空时显示的行内占位。
 *
 * 用虚线边框，读作「内容会出现在这里」而不是一条警告。
 *
 * 默认盒模型是有意定的。有两个调用点需要不同密度（环境面板里更高的盒子、快捷键
 * 列表里无边框的那个），所以 `className` 覆盖默认值——调用方只改不同于默认的那个轴，
 * 虚线边框的语汇保持不变。
 */
export function EmptyHint({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div
      className={cn(
        'rounded-md border border-dashed border-border/60 px-3 py-4 text-center text-[11px] text-muted-foreground',
        className,
      )}
    >
      {children}
    </div>
  );
}
