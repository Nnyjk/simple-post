import type { ReactNode } from 'react';

/**
 * GroupHeader — section sub-title inside the env body.
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
      {icon && (
        <span
          aria-hidden
          className="mt-0.5 inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-background text-muted-foreground"
        >
          {icon}
        </span>
      )}
      <div className="min-w-0">
        <div className="text-[10px] font-semibold uppercase tracking-wider text-foreground">
          {title}
        </div>
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
