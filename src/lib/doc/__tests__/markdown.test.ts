/**
 * Markdown 安全渲染测试。Docs 字段是用户输入的 Markdown，文档又会到处转发，
 * 所以这里只认一件事：原始 HTML 不执行、危险协议不出链接。
 */
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { renderMarkdownDoc } from '../markdown';

const EXECUTABLE_ATTR = /<[a-z][^>]*\son[a-z]+\s*=/i;

test('原始 script 标签被转义成文本', () => {
  const html = renderMarkdownDoc('前言 <script>alert(1)</script> 后记');
  assert.equal(/<script/i.test(html), false);
  assert.equal(html.includes('&lt;script&gt;alert(1)&lt;/script&gt;'), true);
  assert.equal(html.includes('前言'), true);
});

test('带事件处理器的 img 被转义，不构成可执行属性', () => {
  const html = renderMarkdownDoc('<img src=x onerror=alert(1)>');
  assert.equal(EXECUTABLE_ATTR.test(html), false);
  assert.equal(html.includes('&lt;img src=x onerror=alert(1)&gt;'), true);
});

test('javascript: 链接被丢弃，只留下链接文字', () => {
  const html = renderMarkdownDoc('[x](javascript:alert(1))');
  assert.equal(/javascript:/i.test(html), false);
  assert.equal(html.includes('href'), false);
  assert.equal(html.includes('x'), true);
});

test('http / mailto / 相对路径与锚点保留成链接，且禁 referrer', () => {
  const html = renderMarkdownDoc(
    '[a](https://example.com/a) [b](mailto:a@example.com) [c](./other.md) [d](#section)',
  );
  assert.equal(html.includes('href="https://example.com/a"'), true);
  assert.equal(html.includes('href="mailto:a@example.com"'), true);
  assert.equal(html.includes('href="./other.md"'), true);
  assert.equal(html.includes('href="#section"'), true);
  assert.equal((html.match(/rel="noreferrer noopener"/g) ?? []).length, 4);
});

test('伪装成协议的写法也拦下来（大小写与空白）', () => {
  for (const href of [
    'JaVaScRiPt:alert(1)',
    'javascript&#58;alert(1)',
    'jav\u0000ascript:alert(1)',
  ]) {
    const html = renderMarkdownDoc(`[x](${href})`);
    assert.equal(/<a\b/i.test(html), false, href);
    assert.equal(/javascript:/i.test(html.replace(/&#58;/g, '')), false, href);
  }
});

test('图片不渲染外链，只留 alt 文字', () => {
  const html = renderMarkdownDoc('![截图](https://tracker.example.com/p.png)');
  assert.equal(html.includes('tracker.example.com'), false);
  assert.equal(html.includes('截图'), true);
});

test('空说明返回空串，常规 Markdown 仍然可用', () => {
  assert.equal(renderMarkdownDoc(undefined), '');
  assert.equal(renderMarkdownDoc('   '), '');
  const html = renderMarkdownDoc('# 标题\n\n- 一\n- 二\n\n`code` 与 **粗体**');
  assert.equal(html.includes('<h1>标题</h1>'), true);
  assert.equal(html.includes('<li>一</li>'), true);
  assert.equal(html.includes('<code>code</code>'), true);
  assert.equal(html.includes('<strong>粗体</strong>'), true);
});
