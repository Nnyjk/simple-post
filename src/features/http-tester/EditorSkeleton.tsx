/**
 * 两个懒加载编辑器在 chunk 流入前显示的占位。
 *
 * 占位占住编辑器的完整高度：居中 spinner 会让面板塌下去再弹回，读起来像布局抖动。
 */
export function EditorSkeleton() {
  return <div className="h-full w-full animate-pulse bg-muted/30" aria-busy="true" />;
}
