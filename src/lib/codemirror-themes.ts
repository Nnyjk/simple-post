import { EditorView } from '@codemirror/view';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { tags as t } from '@lezer/highlight';

/**
 * CodeMirror light theme + JSON-friendly highlight style.
 *
 * Mirrors the HSL palette in `src/styles/globals.css` so the editor
 * tracks the Tailwind `light` palette without depending on `@uiw/codemirror-theme-*`.
 *
 * JSON tokens (propertyName / string / number / bool / null / punctuation /
 * bracket) get explicit colors so the syntax highlighting survives the
 * `json()` language extension in light mode — CodeMirror's stock `'light'`
 * theme doesn't style these tokens, which is why JSON looked unstyled
 * before this was added.
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
  // JSON keys (left side of `:`) — match the primary brand blue so users
  // can quickly scan property names in large payloads.
  { tag: t.propertyName, color: 'hsl(217 91% 38%)', fontWeight: '600' },
  // String values — warm green, easy on the eyes on a white background.
  { tag: t.string, color: 'hsl(120 45% 30%)' },
  // Numbers — orange so they pop without competing with strings.
  { tag: t.number, color: 'hsl(20 75% 38%)' },
  // Booleans / null — purple, bold so JSON literals stand out.
  { tag: [t.bool, t.null, t.atom], color: 'hsl(280 60% 38%)', fontWeight: '600' },
  // Punctuation / brackets / operators — muted slate, shouldn't compete with values.
  { tag: [t.punctuation, t.bracket, t.operator, t.separator], color: 'hsl(240 6% 40%)' },
  // Comments — slightly muted, italic.
  { tag: [t.lineComment, t.blockComment], color: 'hsl(240 4% 46%)', fontStyle: 'italic' },
]);

/**
 * CodeMirror extension bundle to use in light mode. Pass as the `theme`
 * prop on `<CodeMirror>` (it accepts `string | Extension | Extension[]`).
 */
export const lightCodeTheme = [
  lightEditorTheme,
  syntaxHighlighting(lightJsonHighlight),
];