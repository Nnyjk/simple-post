import { cn } from '@/lib/utils';

/**
 * 双色覆盖进度条：底层是客户端已发送，上层是服务端已接收。
 *
 * 两层画在同一根轨道上而不是并排两根，因为它们是**同一个字节流的前后两端**：并排会让人以为
 * 是两段独立的传输。服务端没回传进度时只有底层，也就是普通的单色进度条。
 *
 * 颜色全走主题 token（轨道 `bg-muted`、客户端 `bg-primary`、服务端 `bg-success`），故深浅
 * 两档都由主题决定。
 */
export function DualProgressBar({
  value,
  overlay,
  label,
  className,
}: {
  /** 0–1 的客户端进度。超出范围的输入被夹到两端。 */
  value: number;
  /** 0–1 的服务端进度；`undefined` = 服务端没有回传。 */
  overlay?: number;
  /** 无障碍名称，说明这一根量的是什么。 */
  label: string;
  className?: string;
}) {
  const client = clamp01(value);
  const server = overlay === undefined ? null : clamp01(overlay);

  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(client * 100)}
      className={cn('relative h-1.5 w-full overflow-hidden rounded-full bg-muted', className)}
    >
      <div
        className="absolute inset-y-0 left-0 rounded-full bg-primary transition-[width] duration-150 ease-out"
        style={{ width: `${client * 100}%` }}
      />
      {server !== null && (
        <div
          className="absolute inset-y-0 left-0 rounded-full bg-success transition-[width] duration-150 ease-out"
          style={{ width: `${server * 100}%` }}
        />
      )}
    </div>
  );
}

/** 0–1 之间的数；`NaN` 按 0 处理（除数为 0 的空文件分片会算出它）。 */
function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(1, Math.max(0, value));
}
