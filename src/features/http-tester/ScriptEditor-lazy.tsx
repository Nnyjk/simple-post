/**
 * 请求脚本编辑器的懒加载边界。与 `BodyEditor-lazy` 同一条理由：CodeMirror 是全应用最大的
 * 单个依赖，静态引入会让它成为入口的静态依赖 —— 而「脚本」不是默认视图。
 *
 * 本包装保持调用点的 props 与组件名不变，只把成本挪到 chunk 之后。
 */
import { Suspense, lazy } from 'react';
import { EditorSkeleton } from './EditorSkeleton';

const ScriptEditorImpl = lazy(() =>
  import('./ScriptEditor').then((m) => ({ default: m.ScriptEditor })),
);

export function ScriptEditor({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (value: string) => void;
  /** 编辑器空时的提示语。 */
  placeholder?: string;
}) {
  return (
    <Suspense fallback={<EditorSkeleton />}>
      <ScriptEditorImpl value={value} onChange={onChange} placeholder={placeholder} />
    </Suspense>
  );
}
