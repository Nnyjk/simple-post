import { forwardRef, useState, type ButtonHTMLAttributes } from 'react';
import { cn, focusRing } from '@/lib/utils';

interface SwitchProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'onChange'> {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}

export const Switch = forwardRef<HTMLButtonElement, SwitchProps>(
  ({ className, checked, onCheckedChange, disabled, ...props }, ref) => {
    // 滑块会在指针下变大：静止 12px、hover 14px、按下 17px。Windows 自带的开关就是
    // 这么做的，也正是这个细节让控件读作一个实体，而不是一个上了样式的 div——
    // 点击之前你就看得见将要发生什么
    const [hovered, setHovered] = useState(false);
    const [pressed, setPressed] = useState(false);
    const thumb = pressed ? 17 : hovered ? 14 : 12;

    return (
      <button
        ref={ref}
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onCheckedChange(!checked)}
        onPointerEnter={() => setHovered(true)}
        onPointerLeave={() => {
          setHovered(false);
          setPressed(false);
        }}
        onPointerDown={() => setPressed(true)}
        onPointerUp={() => setPressed(false)}
        // 40x20：16px 的滑块加 2px 余量，需要更宽的轨道，那 120ms 的行程才不显得局促。
        className={cn(
          'relative inline-flex h-5 w-10 shrink-0 cursor-pointer items-center rounded-full transition-colors duration-150',
          focusRing,
          'disabled:cursor-not-allowed disabled:opacity-50',
          // 关是描边轨道而非填灰轨道：在新的亮度阶梯上，填充的 `--muted` 会被读成
          // 「开着但变暗」
          checked ? 'bg-primary' : 'border border-muted-foreground bg-transparent',
          !checked && hovered && !disabled && 'bg-overlay-hover',
          className,
        )}
        {...props}
      >
        <span
          className={cn(
            'pointer-events-none block rounded-full transition-[width,height,transform] duration-150 ease-out',
            // 位移从轨道量起，所以变大部分保持居中，而不是把滑块推向边缘
            checked ? 'translate-x-[24px]' : 'translate-x-[3px]',
            checked ? 'bg-primary-foreground' : 'bg-muted-foreground',
          )}
          style={{ width: thumb, height: thumb }}
        />
      </button>
    );
  },
);
Switch.displayName = 'Switch';
