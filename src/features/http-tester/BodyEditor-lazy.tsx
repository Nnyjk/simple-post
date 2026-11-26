/**
 * 请求体编辑器的懒加载边界。
 *
 * 编辑器就是 CodeMirror：425 kB 原始 / 139 kB gzip，全应用最大的单个依赖。它必须留在 chunk
 * 边界之后 —— 静态引入会让整个 CodeMirror chunk 成为入口的静态依赖，React 挂上第一像素前
 * 浏览器就得全部下载并解析，尽管 body 标签并不是默认视图。
 *
 * 本包装保持 `HttpTester` 的调用点不变（同 props、同组件名），只把成本挪到 chunk 之后。
 */
import { Suspense, lazy } from 'react';
import type { RequestBody } from '@/types/domain';
import type { FormatResult } from '@/lib/utils';
import type { TabUploadProgress } from '@/stores/slices/ui-response';
import type { VarEntry } from './var-hint';
import { EditorSkeleton } from './EditorSkeleton';

const BodyEditorImpl = lazy(() => import('./BodyEditor').then((m) => ({ default: m.BodyEditor })));

export function BodyEditor({
  value,
  onChange,
  validationError,
  onFormatResult,
  varEntries,
  envName,
  endpointId,
  uploadProgress,
}: {
  value: RequestBody;
  onChange: (body: RequestBody) => void;
  /** 钉住的格式错误，归 HttpTester 所有，见 BodyEditor 的 props。 */
  validationError?: string | null;
  onFormatResult?: (result: FormatResult) => void;
  /** 变量候选：环境变量与动态变量并进同一份列表。 */
  varEntries: VarEntry[];
  /** 当前环境名，显示在候选浮层的标题上。 */
  envName?: string;
  /** 当前标签 id。文件分片的读取状态按它重置：换端点时不该沿用上一个的分片表。 */
  endpointId?: string;
  /** 当前标签这一次发送的传输进度。 */
  uploadProgress?: TabUploadProgress | null;
}) {
  return (
    <Suspense fallback={<EditorSkeleton />}>
      <BodyEditorImpl
        value={value}
        onChange={onChange}
        validationError={validationError}
        onFormatResult={onFormatResult}
        varEntries={varEntries}
        envName={envName}
        endpointId={endpointId}
        uploadProgress={uploadProgress}
      />
    </Suspense>
  );
}
