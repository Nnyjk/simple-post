/**
 * 接口文档编辑器的 lazy 边界。
 *
 * `marked` 未压缩 36 kB / gzip 11 kB，且只有文档页够得着；静态引入它会把 markdown
 * chunk 放进每个人的 entry 关键路径，包括从不打开文档的那些人。
 *
 * 与 `DocsEditor` 同一契约（字符串进、字符串出）；`HttpTester` 无需知道实现现在在
 * chunk 边界之后。
 */
import { Suspense, lazy } from 'react';

const DocsEditorImpl = lazy(() => import('./DocsEditor').then((m) => ({ default: m.DocsEditor })));

export function DocsEditor({
  value,
  onChange,
}: {
  value: string;
  onChange: (docs: string) => void;
}) {
  return (
    <Suspense
      fallback={<div className="h-full w-full animate-pulse bg-muted/30" aria-busy="true" />}
    >
      <DocsEditorImpl value={value} onChange={onChange} />
    </Suspense>
  );
}
