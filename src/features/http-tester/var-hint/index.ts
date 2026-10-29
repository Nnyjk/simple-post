/**
 * 变量注入提示工具 —— 一处规则，各输入栏共用。
 *
 * 组成：
 *  - `varHint.ts`：纯规则（半个变量名、浮层开关、插入时的替换、按键该做什么）
 *  - `VarHintAnchor.ts`：浮层的定位锚点
 *  - `useVarHint.ts`：写回与键盘导航的状态机
 *  - `VarHintPicker.tsx`：候选浮层
 *  - `VarHintInput.tsx`：带提示的普通输入框
 */
export {
  extractPartialVar,
  filterVarEntries,
  isInsideVarBlock,
  replacePartialVar,
  varHintKeyAction,
  type VarEntry,
  type VarHintKeyAction,
  type VarHintKeyEvent,
} from './varHint';
export { elementAnchor, rectAnchor, refAnchor, type VarHintAnchor } from './VarHintAnchor';
export { useVarHint, type UseVarHintOptions, type VarHint } from './useVarHint';
export { VarHintPicker, type VarHintPickerProps } from './VarHintPicker';
export { VarHintInput, type VarHintInputProps } from './VarHintInput';
