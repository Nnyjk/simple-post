import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from 'react';
import { cn } from '@/lib/utils';

export interface InlineEditProps {
  /** 当前已持久化的值——也是取消时回退到的值。 */
  value: string;
  /**
   * 在 Enter / 失焦时以新值（可先 trim）调用，条件是新值非空**且**不同于 `value`。
   * 组件刻意跳过空提交，调用方不必自己做 diff。
   */
  onSave: (next: string) => void;
  /**
   * 展示元素的 render-prop。组件把返回的节点裹进一个 `<span>`，它监听
   * `onDoubleClick` 并转发 `open` 回调，触发器怎么接由调用方决定（按钮、图标、
   * 值文本本身都可以）。
   */
  display: (trigger: { open: () => void }) => ReactNode;
  placeholder?: string;
  className?: string;
  /**
   * 提交/取消前对草稿做 trim。默认 `true` 以对齐树行重命名的交互；
   * 确实允许首尾空白的字段（少见）设为 `false`。
   */
  trim?: boolean;
  ariaLabel?: string;
  testId?: string;
  /**
   * 受控的编辑状态。不传时编辑器自持状态——除树行外的所有调用方都是如此。
   *
   * 树需要另一种模式：它把「正在重命名的行」存为树状态，这样*新建*的行可以直接
   * 进入重命名而无需合成一次双击。与 onEditingChange 配对，由它告知用户已结束。
   */
  editing?: boolean;
  /**
   * 编辑器进入或离开编辑模式时通知——包括由用户触发的改变（双击 / Enter / Esc /
   * 失焦），这使它成为受控 `editing` 的闭合通道。对用它在编辑期间门控其他交互的
   * 父组件也有用（例如把页签标为有未保存改动）。
   */
  onEditingChange?: (editing: boolean) => void;
  /**
   * 为 `true` 时，空草稿会作为 `onSave('')` 提交，而不是当作取消。默认
   * `false`（留空 = 取消）以对齐树行重命名的交互。确实允许清空的字段
   * （如可选描述）设为 `true`。
   */
  commitEmpty?: boolean;
}

/**
 * 通用行内编辑器。默认渲染 display 节点；双击（或经 display 的 open() 回调）
 * 换成自动聚焦并全选原文的单行 <input>。键盘：
 *   - Enter  → 提交（非空且有变化则保存，否则取消——除非 commitEmpty，
 *              此时空草稿会作为 `onSave('')` 提交）
 *   - Esc    → 取消（草稿回退到 `value`）
 *   - Blur   → 提交
 *
 * 编辑模式下 <input> 上的 click 与 Enter 事件不冒泡。这对一个通常嵌在可点
 * 行里（编辑时那行不该折叠）、旁边还有绑 Enter 的兄弟组件（如 TagInput）的行内编辑器
 * 来说是正确默认。确实需要冒泡的调用方可以自己再包一层点击目标。
 */
export function InlineEdit({
  value,
  onSave,
  display,
  placeholder,
  className,
  trim = true,
  ariaLabel,
  testId,
  editing,
  onEditingChange,
  commitEmpty = false,
}: InlineEditProps) {
  const [localEditing, setLocalEditing] = useState(false);
  // 传了 `editing` 就是受控，否则编辑器自持这个标志。判据是
  // `editing === undefined`（而非 falsy），所以传 `editing={false}` 仍表示「受控且关闭」。
  const isEditing = editing ?? localEditing;
  const [draft, setDraft] = useState(value);
  const inputRef = useRef<HTMLInputElement>(null);
  // 防 Enter 在 React 卸载编辑分支时同时触发 keydown 与合成 blur 而重复提交。
  const committedRef = useRef(false);
  const cancelledRef = useRef(false);

  /**
   * 进入/离开编辑模式。受控模式下标志归父组件，所以这里只上报意图——在那里写本地状态
   * 会被父组件的下一次渲染静默覆盖，编辑器看起来会完全没打开。
   */
  const request = useCallback(
    (next: boolean) => {
      if (editing === undefined) setLocalEditing(next);
      onEditingChange?.(next);
    },
    [editing, onEditingChange],
  );

  // 底层 value 在一次编辑会话之外变化时（如树从另一个面板更新），重新同步本地草稿。
  useEffect(() => {
    if (!isEditing) setDraft(value);
  }, [value, isEditing]);

  // 进入编辑模式时自动聚焦并全选。
  useEffect(() => {
    if (!isEditing) return;
    const el = inputRef.current;
    if (!el) return;
    el.focus();
    el.select();
  }, [isEditing]);

  const open = useCallback(() => {
    cancelledRef.current = false;
    committedRef.current = false;
    setDraft(value);
    request(true);
  }, [value, request]);

  const cancel = useCallback(() => {
    cancelledRef.current = true;
    setDraft(value);
    request(false);
  }, [value, request]);

  const commit = useCallback(() => {
    if (committedRef.current) return;
    if (cancelledRef.current) {
      request(false);
      return;
    }
    committedRef.current = true;
    const next = trim ? draft.trim() : draft;
    const shouldSave = commitEmpty ? next !== value : next.length > 0 && next !== value;
    if (shouldSave) onSave(next);
    request(false);
  }, [draft, value, onSave, trim, commitEmpty, request]);

  const handleKey = (e: KeyboardEvent<HTMLInputElement>) => {
    // 阻止 Enter 冒泡——兄弟组件（如 TagInput）可能绑了 Enter，会把已提交的重命名当成自己的动作。
    if (e.key === 'Enter') {
      e.preventDefault();
      e.stopPropagation();
      commit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      cancel();
    }
  };

  const stopClick: React.MouseEventHandler<HTMLInputElement> = (e) => {
    // input 自己吞掉点击，好让外层那行的 onClick（折叠/展开）不会在编辑途中触发。
    e.stopPropagation();
  };

  if (!isEditing) {
    return (
      <span
        onDoubleClick={open}
        // `inline-flex` 让包裹元素与周围文本同行，并让 display 自然决定尺寸。
        className={cn('inline-flex', className)}
      >
        {display({ open })}
      </span>
    );
  }

  return (
    <input
      ref={inputRef}
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onClick={stopClick}
      onKeyDown={handleKey}
      onBlur={commit}
      placeholder={placeholder}
      aria-label={ariaLabel}
      data-testid={testId}
      className={cn(
        'h-6 w-full min-w-0 rounded-sm border border-ring bg-background px-1.5 text-xs text-foreground focus:outline-none',
        className,
      )}
    />
  );
}
