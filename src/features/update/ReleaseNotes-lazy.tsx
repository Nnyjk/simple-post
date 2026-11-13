/**
 * 发布说明的 lazy 边界。`marked` 未压缩 36 kB / gzip 11 kB，且只有更新对话框够得着；
 * 静态引入它会把 markdown 解析器放进每一个冷启动的静态图。
 *
 * 与 `ReleaseNotes` 同一契约（markdown 字符串进、节点出），调用方不必知道实现在 chunk
 * 边界之后。
 */
import { Suspense, lazy } from 'react';

const ReleaseNotesImpl = lazy(() =>
  import('./ReleaseNotes').then((m) => ({ default: m.ReleaseNotes })),
);

export function ReleaseNotes({ notes }: { notes: string }) {
  return (
    <Suspense
      fallback={
        <div className="h-20 w-full animate-pulse rounded-md bg-muted/30" aria-busy="true" />
      }
    >
      <ReleaseNotesImpl notes={notes} />
    </Suspense>
  );
}
