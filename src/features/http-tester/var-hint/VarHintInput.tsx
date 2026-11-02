import { useCallback, useMemo, useRef, type InputHTMLAttributes, type Ref } from 'react';
import { Input } from '@/components/ui/input';
import { refAnchor } from './VarHintAnchor';
import { VarHintPicker } from './VarHintPicker';
import { useVarHint } from './useVarHint';
import type { VarEntry } from './varHint';

export type VarHintInputProps = Omit<
  InputHTMLAttributes<HTMLInputElement>,
  'value' | 'onChange' | 'onKeyDown'
> & {
  /** 字段当前文本。 */
  value: string;
  /** 写回字段文本。 */
  onChange: (next: string) => void;
  /** 候选条目：环境变量与动态变量并进同一份列表。 */
  entries: VarEntry[];
  /** 浮层头部显示的环境名。 */
  envName?: string;
  /** 调用方的 ref。组件把同一个元素也交给浮层作定位锚点。 */
  inputRef?: Ref<HTMLInputElement>;
};

/**
 * 带变量候选提示的输入框。
 *
 * 用户敲下 `{{` 时浮出候选，选中一项即把 `{{key}}` 写进字段并把光标放到 token 末尾。
 * 浮层的位置由 `refAnchor` 读取输入框的屏幕坐标，故输入框在页面里挪动时浮层跟着走。
 */
export function VarHintInput({
  value,
  onChange,
  entries,
  envName,
  inputRef,
  className,
  ...rest
}: VarHintInputProps) {
  const innerRef = useRef<HTMLInputElement | null>(null);

  // 调用方的 ref 与组件自己的 ref 都要拿到同一个元素：前者供调用方聚焦，后者供浮层定位。
  const setRef = useCallback(
    (el: HTMLInputElement | null) => {
      innerRef.current = el;
      if (typeof inputRef === 'function') inputRef(el);
      else if (inputRef) inputRef.current = el;
    },
    [inputRef],
  );

  const anchor = useMemo(() => refAnchor(innerRef), []);

  const hint = useVarHint({
    value,
    onChange,
    // 受控输入框的值由 React 在下一次渲染时写回，那次写回会把光标挪到末尾，故把光标
    // 定位延后一拍，等写回之后再落。
    onCaret: (caret) => {
      window.setTimeout(() => {
        const el = innerRef.current;
        if (!el) return;
        el.focus();
        try {
          el.setSelectionRange(caret, caret);
        } catch {
          /* 不是文本输入框 —— 无内容可选 */
        }
      }, 0);
    },
    entries,
  });

  return (
    <>
      <Input
        ref={setRef}
        {...rest}
        value={value}
        onChange={(e) => hint.change(e.target.value)}
        onKeyDown={hint.keyDown}
        className={className}
      />
      <VarHintPicker
        open={hint.open}
        onClose={hint.close}
        entries={hint.entries}
        selected={hint.selected}
        onSelectedChange={hint.setSelected}
        onSelect={hint.insert}
        anchor={anchor}
        envName={envName}
      />
    </>
  );
}
