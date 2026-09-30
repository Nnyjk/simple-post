import { forwardRef, type InputHTMLAttributes } from 'react';
import { cn, focusRing } from '@/lib/utils';

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  ({ className, type, ...props }, ref) => {
    return (
      <input
        ref={ref}
        type={type}
        className={cn(
          'flex h-8 w-full rounded-md border border-input bg-background px-2.5 py-1 text-sm',
          'placeholder:text-muted-foreground/60',
          focusRing,
          'disabled:cursor-not-allowed disabled:opacity-50',
          className,
        )}
        {...props}
      />
    );
  },
);
Input.displayName = 'Input';
