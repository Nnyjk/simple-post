/**
 * 发布说明的 markdown 渲染。与 `DocsEditor` 的预览同一套样式类（`.markdown-prose`），
 * 故发布说明里的标题、列表、代码块与接口文档读起来是同一种排版。
 *
 * 文本来源是仓库所有者写在 release 上的说明，经 `latest.json` / GitHub 接口到达前端。
 * `marked` 不转义 markdown 里的原样 HTML，故这个组件只接这份说明，不接任何用户输入。
 */
import { useMemo } from 'react';
import { marked } from 'marked';

export function ReleaseNotes({ notes }: { notes: string }) {
  const html = useMemo(() => marked.parse(notes, { async: false }) as string, [notes]);
  return (
    <article
      className="markdown-prose text-xs leading-relaxed"
      // `marked.parse` 对上述来源的输出。
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
