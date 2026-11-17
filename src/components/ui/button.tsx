import { forwardRef, type ButtonHTMLAttributes } from 'react';
import { cn, focusRing } from '@/lib/utils';

export type ButtonVariant = 'default' | 'ghost' | 'outline' | 'secondary' | 'destructive';
export type ButtonSize = 'default' | 'sm' | 'xs' | 'icon';

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

/** 悬停底色是 `bg-overlay-hover`（3.5% 黑 / 5% 白），与树行、标签页、菜单项同一族；
 *  `bg-accent` 留给「已选中」。两档实测：浅色档 accent 91.8% 与 overlay 叠在白底上的
 *  ~96.4% 相差一倍多，行列表里 accent 那档会把 30 行看成棋盘，故统一走轻的那一档 ——
 *  悬停时文字颜色同时挪到 `accent-foreground`，可辨识度由字色承担。 */
const variantClass: Record<ButtonVariant, string> = {
  default: 'bg-primary text-primary-foreground hover:bg-primary/90 shadow-sm',
  ghost: 'hover:bg-overlay-hover hover:text-accent-foreground',
  outline:
    'border border-border bg-transparent hover:bg-overlay-hover hover:text-accent-foreground',
  secondary: 'bg-secondary text-secondary-foreground hover:bg-secondary/80',
  destructive: 'bg-destructive text-destructive-foreground hover:bg-destructive/90',
};

const sizeClass: Record<ButtonSize, string> = {
  default: 'h-9 px-4 text-sm',
  sm: 'h-8 px-3 text-sm',
  xs: 'h-7 px-2 text-xs',
  icon: 'h-9 w-9 p-0',
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = 'default', size = 'default', type = 'button', ...props }, ref) => {
    return (
      <button
        ref={ref}
        type={type}
        className={cn(
          'inline-flex items-center justify-center gap-1.5 rounded-md font-medium transition-colors',
          focusRing,
          'disabled:pointer-events-none disabled:opacity-50',
          variantClass[variant],
          sizeClass[size],
          className,
        )}
        {...props}
      />
    );
  },
);
Button.displayName = 'Button';
