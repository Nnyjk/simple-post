/**
 * 发布说明的 markdown 渲染。与 `DocsEditor` 的预览同一套样式类（`.markdown-prose`），
 * 故发布说明里的标题、列表、代码块与接口文档读起来是同一种排版。
 *
 * 文本来源是仓库所有者写在 release 上的说明，经 `latest.json` / GitHub 接口到达前端。
 * `marked` 不转义 markdown 里的原样 HTML，故这个组件只接这份说明，不接任何用户输入。
 *
 * 说明里的链接同样走所选镜像：markdown 渲染出的 `<a>` 不在 React 的控制里，故在渲染后
 * 逐个改写 `href`。原文另存在 `WeakMap` 里，换镜像时从原文重算 —— 直接从已改写的
 * `href` 再改一次，前缀镜像会层层套起来。
 */
import { useEffect, useMemo, useRef } from 'react';
import { marked } from 'marked';
import { mirrorGithubUrl, type GithubMirrorId } from '@/lib/github-mirror';

const originalHrefs = new WeakMap<Element, string>();

export function ReleaseNotes({ notes, mirror }: { notes: string; mirror?: GithubMirrorId }) {
  const html = useMemo(() => marked.parse(notes, { async: false }) as string, [notes]);
  const ref = useRef<HTMLElement>(null);

  useEffect(() => {
    const root = ref.current;
    if (!root) return;
    const target = mirror ?? 'direct';
    for (const anchor of root.querySelectorAll('a[href]')) {
      const original = originalHrefs.get(anchor) ?? anchor.getAttribute('href') ?? '';
      originalHrefs.set(anchor, original);
      anchor.setAttribute('href', mirrorGithubUrl(original, target));
      anchor.setAttribute('target', '_blank');
      anchor.setAttribute('rel', 'noopener noreferrer');
    }
  }, [notes, mirror]);

  return (
    <article
      ref={ref}
      className="markdown-prose text-xs leading-relaxed"
      // `marked.parse` 对上述来源的输出。
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
