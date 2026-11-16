/**
 * Docs / 备注字段的 Markdown 渲染 —— 面向「一份会被到处转发的文件」的收紧版。
 *
 * 用 `marked`（工作区里本来就有，Docs 编辑器用的也是它），但把渲染器的三个危险出口全换掉：
 *
 *   - 原始 HTML（`html` token）：当作**文本**转义输出。文档里出现 `<script>` 只说明作者写了这几个
 *     字符，不该变成标签。
 *   - 链接（`link` token）：协议白名单 `http` / `https` / `mailto` / 相对路径 / `#` 锚点；不认的
 *     一律退化成纯文本（`[x](javascript:alert(1))` 渲染成 `x`，连 href 都不留）。`title` 与
 *     自定义属性一概丢弃。
 *   - 图片（`image` token）：只输出 alt 文本。快照要求零外链，`data:` 图片又会把文件撑大，
 *     两种都不该出现。
 *
 * 其余 token（代码块、表格、列表……）沿用 marked 的默认实现，它们自己会转义文本。
 */
import { marked, Renderer, type Tokens } from 'marked';
import { escapeHref, escapeHtml, isSafeHref } from './escape';

class SafeRenderer extends Renderer {
  override html({ text }: Tokens.HTML | Tokens.Tag): string {
    return escapeHtml(text);
  }

  override link({ href, tokens }: Tokens.Link): string {
    const inner = this.parser.parseInline(tokens);
    if (!isSafeHref(href)) return inner;
    return `<a href="${escapeHref(href)}" rel="noreferrer noopener" target="_blank">${inner}</a>`;
  }

  override image({ text }: Tokens.Image): string {
    return escapeHtml(text);
  }
}

/** 把一段 Markdown 渲染成**已转义**的 HTML 片段（调用方直接嵌进页面即可）。空串得到空串。 */
export function renderMarkdownDoc(source: string | undefined): string {
  const text = source?.trim();
  if (!text) return '';
  return marked.parse(text, { async: false, gfm: true, renderer: new SafeRenderer() }) as string;
}
