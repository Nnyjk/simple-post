import { forwardRef, useState, type ButtonHTMLAttributes } from 'react';
import { cn, focusRing } from '@/lib/utils';

interface SwitchProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'onChange'> {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}

export const Switch = forwardRef<HTMLButtonElement, SwitchProps>(
  ({ className, checked, onCheckedChange, disabled, ...props }, ref) => {
    // The thumb grows under the cursor: 12px resting, 14px on hover, 17px
    // while pressed. Windows' own switch does this, and it is the detail
    // that makes the control read as a physical object rather than as a
    // styled div — you can see what is about to happen before you click.
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
        // 40x20, not the old 36x20 — a 16px thumb with 2px of breathing room
        // needs the wider track to keep the 120ms travel from looking cramped.
        className={cn(
          'relative inline-flex h-5 w-10 shrink-0 cursor-pointer items-center rounded-full transition-colors duration-150',
          focusRing,
          'disabled:cursor-not-allowed disabled:opacity-50',
          // Off is an outlined track, not a filled grey one: on the new
          // ladder a filled `--muted` read as "on but dim".
          checked ? 'bg-primary' : 'border border-muted-foreground bg-transparent',
          !checked && hovered && !disabled && 'bg-overlay-hover',
          className,
        )}
        {...props}
      >
        <span
          className={cn(
            'pointer-events-none block rounded-full transition-[width,height,transform] duration-150 ease-out',
            // Position is measured from the track, so the growth stays
            // centred rather than pushing the thumb toward the edge.
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
