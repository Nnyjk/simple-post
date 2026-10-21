/**
 * 请求脚本编辑器的懒加载边界。
 *
 * 与 `BodyEditor-lazy` 同一条理由：CodeMirror 是全应用最大的单个依赖，静态引入会让它
 * 成为入口的静态依赖，React 挂上第一像素前浏览器就得全部下载并解析——而「脚本」不是
 * 默认视图。
 *
 * 本包装保持调用点的 props 与组件名不变，只把成本挪到 chunk 之后。
 */
import { Suspense, lazy } from 'react';

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

/** chunk 流入期间占住编辑器的完整高度。居中 spinner 会让面板塌下去再弹回，读起来像布局抖动。 */
function EditorSkeleton() {
  return <div className="h-full w-full animate-pulse bg-muted/30" aria-busy="true" />;
}
