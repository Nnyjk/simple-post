import type { ReactNode } from 'react';
import { IconTile } from '@/components/ui/icon-tile';

/**
 * GroupHeader —— 环境内容里的分区副标题。
 */
export function GroupHeader({
  icon,
  title,
  description,
  meta,
}: {
  icon?: ReactNode;
  title: string;
  description?: string;
  meta?: string;
}) {
  return (
    <div className="flex items-start gap-2 border-b border-border/60 bg-muted/20 px-4 py-2">
      {icon && <IconTile size="sm" className="mt-0.5 bg-background" icon={icon} />}
      <div className="min-w-0">
        <div className="label-caps-strong">{title}</div>
        {description && (
          <div className="mt-0.5 text-[10px] text-muted-foreground">
            {description}
            {meta ? (
              <>
                {' · '}
                <span className="font-mono">{meta}</span>
              </>
            ) : null}
          </div>
        )}
      </div>
    </div>
  );
}
