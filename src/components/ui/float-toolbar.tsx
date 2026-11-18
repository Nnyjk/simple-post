import type { ReactNode } from 'react';
import { Button } from './button';
import { Tooltip } from './tooltip';
import { LAYER } from './layers';
import { cn } from '@/lib/utils';

export interface FloatToolbarAction {
  /** 稳定的键，同时用作 React 的 key。 */
  id: string;
  /** 按钮的提示文案。 */
  label: string;
  /** 按钮处于开启态时替换 `label` 的提示文案。 */
  activeLabel?: string;
  icon: ReactNode;
  onClick: () => void;
  /** 开启态：按钮留下按下底色，提示改用 `activeLabel`。 */
  active?: boolean;
  disabled?: boolean;
  testId?: string;
}

export interface FloatToolbarProps {
  actions: FloatToolbarAction[];
  className?: string;
}

/**
 * 浮在内容区右上角的一排小图标按钮。
 *
 * 它只负责定位与提示，按钮的语义全在 `actions` 里 —— 响应体与请求体两侧因此共用同一
 * 个工具条，而不是各写一份。宿主容器必须是 `relative`；工具条浮在内容右上角之上。
 */
export function FloatToolbar({ actions, className }: FloatToolbarProps) {
  return (
    <div
      data-testid="float-toolbar"
      className={cn(
        'absolute right-2 top-2 flex items-center gap-0.5 rounded-md border border-border bg-card/90 p-0.5 shadow-sm backdrop-blur',
        LAYER.popover,
        className,
      )}
    >
      {actions.map((action) => {
        const hint = action.active ? (action.activeLabel ?? action.label) : action.label;
        return (
          <Tooltip key={action.id} content={hint} side="bottom">
            <Button
              variant="ghost"
              size="icon"
              className={cn('h-7 w-7', action.active && 'bg-overlay-hover text-accent-foreground')}
              onClick={action.onClick}
              disabled={action.disabled}
              aria-pressed={action.active}
              data-testid={action.testId}
            >
              {action.icon}
            </Button>
          </Tooltip>
        );
      })}
    </div>
  );
}
