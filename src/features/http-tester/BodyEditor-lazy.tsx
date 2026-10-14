/**
 * 请求体编辑器的懒加载边界。
 *
 * 编辑器就是 CodeMirror：425 kB 原始 / 139 kB gzip，全应用最大的单个依赖。它必须留在
 * chunk 边界之后 —— 静态引入会让整个 CodeMirror chunk 成为入口的静态依赖，React 挂上
 * 第一像素前浏览器就得全部下载并解析，尽管 body 标签并不是默认视图。
 *
 * 本包装保持 `HttpTester` 的调用点不变（同 props、同组件名），只把成本挪到 chunk 之后。
 * 该 chunk 仍在 `App.tsx` 的 bootstrap 窗口里预热，故常见路径（回访用户恢复了接口标签）
 * 到这个标签可见时通常已经解析完。
 */
import { Suspense, lazy } from 'react';
import type { RequestBody } from '@/types/domain';
import type { FormatResult } from '@/lib/utils';

const BodyEditorImpl = lazy(() => import('./BodyEditor').then((m) => ({ default: m.BodyEditor })));

export function BodyEditor({
  value,
  onChange,
  validationError,
  onFormatResult,
}: {
  value: RequestBody;
  onChange: (body: RequestBody) => void;
  /** 钉住的格式错误，归 HttpTester 所有，见 BodyEditor 的 props。 */
  validationError?: string | null;
  onFormatResult?: (result: FormatResult) => void;
}) {
  return (
    <Suspense fallback={<EditorSkeleton />}>
      <BodyEditorImpl
        value={value}
        onChange={onChange}
        validationError={validationError}
        onFormatResult={onFormatResult}
      />
    </Suspense>
  );
}

/** chunk 流入期间占住编辑器的完整高度。居中 spinner 会让面板塌下去再弹回，读起来像布局抖动。 */
function EditorSkeleton() {
  return <div className="h-full w-full animate-pulse bg-muted/30" aria-busy="true" />;
}
