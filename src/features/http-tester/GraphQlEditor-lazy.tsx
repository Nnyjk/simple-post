/**
 * GraphQL 编辑器的懒加载边界。与 `ScriptEditor-lazy` 同一条理由：CodeMirror 是全应用最大的
 * 单个依赖，静态引入会让它成为入口的静态依赖 —— 而「GraphQL」不是默认视图。
 *
 * 本包装保持调用点的 props 与组件名不变，只把成本挪到 chunk 之后。
 */
import { Suspense, lazy } from 'react';
import { EditorSkeleton } from './EditorSkeleton';
import type { GraphQlCompletionSource } from './GraphQlEditor';

const GraphQlEditorImpl = lazy(() =>
  import('./GraphQlEditor').then((m) => ({ default: m.GraphQlEditor })),
);

export function GraphQlEditor({
  value,
  onChange,
  language,
  height,
  placeholder,
  completion,
}: {
  value: string;
  onChange: (value: string) => void;
  language: 'graphql' | 'json';
  height: string;
  placeholder?: string;
  completion?: GraphQlCompletionSource;
}) {
  return (
    <Suspense fallback={<EditorSkeleton />}>
      <GraphQlEditorImpl
        value={value}
        onChange={onChange}
        language={language}
        height={height}
        placeholder={placeholder}
        completion={completion}
      />
    </Suspense>
  );
}
