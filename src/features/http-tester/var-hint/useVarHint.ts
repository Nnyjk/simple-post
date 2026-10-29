import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react';
import {
  extractPartialVar,
  filterVarEntries,
  isInsideVarBlock,
  replacePartialVar,
  varHintKeyAction,
  type VarEntry,
} from './varHint';

/**
 * 变量提示的状态与处理器。
 *
 * 字段的当前文本由调用方给，写回也由调用方做——本 hook 不碰 store，也不持有字段本身。
 */
export interface VarHint {
  /** 浮层是否该出现。 */
  open: boolean;
  /** 按用户输入的片段过滤后的候选。 */
  entries: VarEntry[];
  /** 当前高亮的候选下标。 */
  selected: number;
  setSelected: (next: number) => void;
  /** 关掉浮层。 */
  close: () => void;
  /** 字段文本变了：写回并更新浮层开关。 */
  change: (next: string) => void;
  /**
   * 文本被别处改成了这样：只更新浮层开关，不写回。
   *
   * CodeMirror 的文档是编辑器自己改的，文本先到组件、再由组件写回 store，故那条路径
   * 不能用 `change`（它会写回一次刚被编辑器写过的文本）。
   */
  sync: (next: string) => void;
  /** 键盘导航与确认。 */
  keyDown: (e: ReactKeyboardEvent) => void;
  /** 插入一个变量。 */
  insert: (key: string) => void;
}

export interface UseVarHintOptions {
  /** 字段当前文本。 */
  value: string;
  /** 写回字段文本。 */
  onChange: (next: string) => void;
  /**
   * 插入完成后把光标放到变量 token 的末尾。
   *
   * 调用方负责移动光标：普通输入框用 `setSelectionRange`，CodeMirror 用一次选区变更。
   */
  onCaret?: (caret: number) => void;
  /** 候选条目：环境变量与动态变量并进同一份列表。 */
  entries: VarEntry[];
  /**
   * 光标在 `value` 里的下标。
   *
   * 缺省是文本末尾，故普通输入框不必给；CodeMirror 给出真实光标位置，于是「半个变量名」
   * 取的是光标之前的那一段，而不是整篇文档的末尾。
   */
  caret?: number;
}

/**
 * 变量注入提示的状态机。
 *
 * 浮层的开关跟着文本走：文本里没有未闭合的 `{{` 时它就该消失，用户把 `{{` 敲完整了它也
 * 该消失（那已经是写好的变量，不再需要候选）。
 */
export function useVarHint({
  value,
  onChange,
  onCaret,
  entries,
  caret,
}: UseVarHintOptions): VarHint {
  const [open, setOpen] = useState(false);
  const [selected, setSelected] = useState(0);

  const caretPos = caret ?? value.length;
  const before = value.slice(0, caretPos);
  const after = value.slice(caretPos);

  const partial = extractPartialVar(before);
  const filtered = useMemo(() => filterVarEntries(entries, partial), [entries, partial]);

  // 候选变少时把高亮收回范围，否则键盘导航会停在一个已经不存在的行上。
  useEffect(() => {
    if (filtered.length === 0) return;
    if (selected < 0 || selected >= filtered.length) setSelected(0);
  }, [filtered.length, selected]);

  const close = useCallback(() => setOpen(false), []);

  const change = useCallback(
    (next: string) => {
      onChange(next);
      const inside = isInsideVarBlock(next);
      setOpen(inside);
      // 浮层关掉时把高亮归零：下次打开时用户看到的是第一项，而不是上一次残留的位置。
      if (!inside) setSelected(0);
    },
    [onChange],
  );

  const sync = useCallback((next: string) => {
    const inside = isInsideVarBlock(next);
    setOpen(inside);
    if (!inside) setSelected(0);
  }, []);

  const insert = useCallback(
    (key: string) => {
      const next = replacePartialVar(before, key);
      // 没有可替换的片段时不动字段：浮层可能是在文本已经变了之后才关掉的。
      if (next === before) return;
      onChange(next + after);
      setOpen(false);
      setSelected(0);
      // 光标落在替换掉的那一段末尾，也就是刚插入的 `{{key}}` 之后。
      onCaret?.(next.length);
    },
    [after, before, onCaret, onChange],
  );

  const keyDown = useCallback(
    (e: ReactKeyboardEvent) => {
      const entry = filtered[selected] ?? filtered[0];
      const action = varHintKeyAction(
        {
          key: e.key,
          ctrlKey: e.ctrlKey,
          metaKey: e.metaKey,
          altKey: e.altKey,
          isComposing: e.nativeEvent.isComposing,
          keyCode: e.keyCode,
        },
        { open, hasEntry: entry !== undefined },
      );
      if (action === 'none') return;
      e.preventDefault();
      if (action === 'next') {
        if (filtered.length > 0) setSelected((s) => (s + 1) % filtered.length);
      } else if (action === 'prev') {
        if (filtered.length > 0) setSelected((s) => (s - 1 + filtered.length) % filtered.length);
      } else if (action === 'insert') {
        if (entry) insert(entry.key);
      } else if (action === 'close') {
        setOpen(false);
      }
    },
    [filtered, insert, open, selected],
  );

  return {
    open,
    entries: filtered,
    selected,
    setSelected,
    close,
    change,
    sync,
    keyDown,
    insert,
  };
}
