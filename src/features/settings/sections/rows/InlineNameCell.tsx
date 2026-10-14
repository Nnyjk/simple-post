import { useEffect, useRef, useState } from 'react';
import { Pencil } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';

/**
 * 紧凑的「点击即编辑」文本单元格。点值换成输入框；失焦或回车提交，Esc 还原。可选的
 * `validate` 会拦下提交并就地显示错误。
 *
 * 刻意留在环境分区本地（而非复用 `@/components/ui/InlineEdit`）：它单击即开 ——
 * 这里行距很密的表格布局容不下双击的手势提示；且它支持 `validate` 与就地报错，
 * `InlineEdit` 没有对应能力。
 *
 * `autoEdit` 让输入框不经点击直接打开。环境面板用它把刚建好的环境直接落到命名上，
 * 用户不必去找重命名入口；其余调用点（服务名、变量键）仍由点击驱动打开。
 */
export function InlineNameCell({
  value,
  placeholder,
  validate,
  onCommit,
  autoEdit,
  onAutoEditEnd,
  className,
}: {
  value: string;
  placeholder: string;
  validate?: (next: string) => string | null;
  onCommit: (next: string) => void;
  /**
   * 挂载时 / 该标志由 false 翻成 true 时以重命名模式打开。消费掉之后由属主经
   * `onAutoEditEnd` 把它清掉。
   */
  autoEdit?: boolean;
  /** `autoEdit` 的这次打开一被消费就触发，好让属主落下标志。 */
  onAutoEditEnd?: () => void;
  className?: string;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  // `autoEdit` 是标志而非事件：它可以跨多次重渲染一直为 `true`，单元格之后也可能重新
  // 挂载（例如切环境会给正文换 key）。用 ref 上闩把它变成 false → true 的边沿，
  // 于是每次拉高只开一次，而不会在每次击键引起的重渲染上都开。
  const autoOpened = useRef(false);
  // 只有自动打开才预选文本。点击编辑既有行仍保持原先「光标在末尾」的行为，
  // 免得改个服务名悄悄变成全选覆盖。
  const preselectOnOpen = useRef(false);

  useEffect(() => {
    if (!autoEdit) {
      autoOpened.current = false;
      return;
    }
    if (autoOpened.current) return;
    autoOpened.current = true;
    preselectOnOpen.current = true;
    setDraft(value);
    setError(null);
    setEditing(true);
    onAutoEditEnd?.();
  }, [autoEdit, value, onAutoEditEnd]);

  // 在 `editing` 翻过去之后的那一次渲染才跑，那才是输入框真正存在的第一刻 ——
  // 上面的 effect 够不到这个 ref。
  useEffect(() => {
    if (!editing || !preselectOnOpen.current) return;
    preselectOnOpen.current = false;
    inputRef.current?.select();
  }, [editing]);

  const begin = () => {
    setDraft(value);
    setError(null);
    setEditing(true);
  };
  const cancel = () => {
    setEditing(false);
    setError(null);
  };
  const commit = () => {
    const trimmed = draft.trim();
    if (validate) {
      const msg = validate(trimmed);
      if (msg) {
        setError(msg);
        return;
      }
    }
    if (trimmed !== value) onCommit(trimmed);
    setEditing(false);
    setError(null);
  };

  if (!editing) {
    return (
      <button
        type="button"
        onClick={begin}
        className={cn(
          'group/cell flex w-full items-center gap-1 rounded px-1.5 py-0.5 text-left transition-colors hover:bg-accent/40',
          !value && 'text-muted-foreground/60 italic',
          className,
        )}
      >
        <span className="min-w-0 truncate">{value || placeholder}</span>
        <Pencil className="h-3 w-3 shrink-0 opacity-0 transition-opacity group-hover/cell:opacity-100" />
      </button>
    );
  }

  return (
    <div className="flex flex-col gap-1">
      <Input
        ref={inputRef}
        autoFocus
        value={draft}
        onChange={(e) => {
          setDraft(e.target.value);
          if (error) setError(null);
        }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            commit();
          } else if (e.key === 'Escape') {
            e.preventDefault();
            cancel();
          }
        }}
        className="h-7 font-mono text-xs"
      />
      {error && <div className="px-1 text-[10px] text-destructive">{error}</div>}
    </div>
  );
}
