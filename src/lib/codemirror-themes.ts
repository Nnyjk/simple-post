import { EditorView } from '@codemirror/view';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { tags as t } from '@lezer/highlight';

/**
 * CodeMirror 亮色主题 + 偏 JSON 的高亮样式。
 *
 * 镜像 `src/styles/globals.css` 里的 HSL 调色板，好让编辑器跟上 Tailwind 的 `light`
 * 调色板，又不依赖 `@uiw/codemirror-theme-*`。
 *
 * JSON token（propertyName / string / number / bool / null / punctuation / bracket）
 * 逐个指定颜色，这样亮色模式下的 `json()` 语言扩展仍有语法高亮——CodeMirror 自带的
 * `'light'` 主题不给这些 token 上色。
 */
const lightEditorTheme = EditorView.theme(
  {
    '&': {
      backgroundColor: 'hsl(0 0% 100%)',
      color: 'hsl(240 10% 3.9%)',
    },
    '.cm-content': {
      caretColor: 'hsl(217 91% 60%)',
    },
    '.cm-cursor': {
      borderLeftColor: 'hsl(217 91% 60%)',
    },
    '.cm-gutters': {
      backgroundColor: 'hsl(240 4.8% 95.9%)',
      color: 'hsl(240 3.8% 46.1%)',
      border: 'none',
    },
    '.cm-activeLine': {
      backgroundColor: 'hsl(240 4.8% 95.9% / 0.5)',
    },
    '.cm-activeLineGutter': {
      backgroundColor: 'hsl(240 4.8% 92%)',
      color: 'hsl(240 6% 10%)',
    },
    '&.cm-focused .cm-selectionBackground, ::selection': {
      backgroundColor: 'hsl(217 91% 60% / 0.18)',
    },
    '.cm-selectionBackground': {
      backgroundColor: 'hsl(217 91% 60% / 0.12)',
    },
  },
  { dark: false },
);

const lightJsonHighlight = HighlightStyle.define([
  // JSON 键（`:` 左边）——取主品牌蓝，用户扫大 payload 时能一眼扫出属性名
  { tag: t.propertyName, color: 'hsl(217 91% 38%)', fontWeight: '600' },
  // 字符串值——暖绿，白底上不刺眼
  { tag: t.string, color: 'hsl(120 45% 30%)' },
  // 数字——橙色，跳出来但不与字符串争
  { tag: t.number, color: 'hsl(20 75% 38%)' },
  // 布尔 / null——紫色加粗，让 JSON 字面量突出
  { tag: [t.bool, t.null, t.atom], color: 'hsl(280 60% 38%)', fontWeight: '600' },
  // 标点 / 括号 / 运算符——灰蓝，不与值争
  { tag: [t.punctuation, t.bracket, t.operator, t.separator], color: 'hsl(240 6% 40%)' },
  // 注释——稍灰，斜体
  { tag: [t.lineComment, t.blockComment], color: 'hsl(240 4% 46%)', fontStyle: 'italic' },
]);

/** 亮色模式下用的 CodeMirror 扩展包。作为 `<CodeMirror>` 的 `theme` 属性传入（它接受 `string | Extension | Extension[]`）。 */
export const lightCodeTheme = [lightEditorTheme, syntaxHighlighting(lightJsonHighlight)];
