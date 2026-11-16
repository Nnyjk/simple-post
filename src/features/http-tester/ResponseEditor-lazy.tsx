/**
 * 响应定义编辑器的懒加载边界。
 *
 * 编辑器是 CodeMirror（全应用最大的单个依赖），与 `BodyEditor-lazy` 同理：静态引入会让
 * 它在 React 挂上第一像素之前就下载并解析，而「响应」页签并不是默认视图。
 *
 * 本包装保持 `RequestTabPanels` 的调用点不变（同 props、同组件名），只把成本挪到 chunk 之后。
 */
import { Suspense, lazy } from 'react';
import type { ResponseDefinition } from '@/types/domain';
import { EditorSkeleton } from './EditorSkeleton';

const ResponseEditorImpl = lazy(() =>
  import('./ResponseEditor').then((m) => ({ default: m.ResponseEditor })),
);

export function ResponseEditor({
  value,
  onChange,
}: {
  value: ResponseDefinition[];
  onChange: (responses: ResponseDefinition[]) => void;
}) {
  return (
    <Suspense fallback={<EditorSkeleton />}>
      <ResponseEditorImpl value={value} onChange={onChange} />
    </Suspense>
  );
}
