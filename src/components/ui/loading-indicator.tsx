import { Loader2 } from 'lucide-react';
import { cn } from '@/lib/utils';

/**
 * 加载指示器的两档强度，区分二者的理由是布局是否已经塌，不是尺寸。
 * `LoadingState` 占满所在槽位，用于要挂的子树 / chunk 还没到，不占满就会先塌成一行字再猛地
 * 撑开，是一次布局跳动。
 * `LoadingChip` 绝对定位且 `pointer-events-none`，用于内容已在、数据未到，不占布局也不吃点击；
 * 水平居中是为了避开请求页签条右端的 `Ctrl+Enter` 提示。
 * 两者共用 `Loader2` + `animate-spin`。广播元素不标 `aria-busy` —— 它们在移除之前一直存在，
 * 标上忙碌后辅助技术不会播报标签；忙碌标记由消费方挂在内容区容器上。
 *
 * 消费方是 `Suspense` 的 `fallback` 时不挂 `aria-busy`：那种场合子树还没到，容器本身就是指示
 * 器，没有第二个元素可挂。
 */

interface LoadingIndicatorProps {
  /** 缺省为「加载中…」。传值时必须说明等的是什么，不要只写「请稍候」。 */
  label?: string;
  className?: string;
}

/** 整槽位的加载占位。见模块头部对两档强度分工的说明。 */
export function LoadingState({ label, className }: LoadingIndicatorProps) {
  return (
    <div
      role="status"
      className={cn(
        'flex min-h-0 flex-1 flex-col items-center justify-center gap-2 text-xs text-muted-foreground',
        className,
      )}
    >
      <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
      <span className="opacity-70">{label ?? '加载中…'}</span>
    </div>
  );
}

/** 内容已就位时的轻量角标。见模块头部对两档强度分工的说明。 */
export function LoadingChip({ label, className }: LoadingIndicatorProps) {
  return (
    <div
      role="status"
      className={cn(
        'pointer-events-none absolute left-1/2 top-2 flex -translate-x-1/2 items-center gap-1.5 rounded-md border border-border/60 bg-card/85 px-2 py-1 text-micro text-muted-foreground shadow-sm',
        className,
      )}
    >
      <Loader2 className="h-3 w-3 animate-spin" aria-hidden />
      <span>{label ?? '加载中…'}</span>
    </div>
  );
}
