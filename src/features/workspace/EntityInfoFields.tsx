/**
 * AutoSaveField —— 每个实体「基础信息」层背后的表单原语。
 *
 * 为何自动保存而不是保存按钮
 * -------------------------
 * 这些字段经 `updateProject` / `updateNode` 写入、立即持久化，应用里其他每个配置面也早
 * 就是这个行为（设置面板里明说了：设置改动会立即保存）。在这里放一对「编辑 / 保存」等于
 * 暗示存在一个其实并不存在的暂存区，还会让工作区成为唯一一个关标签就可能丢字的地方。
 * 故：失焦提交、回车提交、Esc 还原，值没变则什么都不写。
 *
 * 草稿存在本地，而让它安全的那一条规则就是重新同步的 effect：来自*外部*的值（树里的改名、
 * store 追上来）必须替换草稿，但只在字段没被输入时 —— 否则一次后台写入会吃掉光标下的字符。
 */

import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';

interface AutoSaveInputProps {
  value: string;
  /** 值有变化时，带着 trim 后的新值调用。 */
  onSave: (next: string) => void;
  placeholder?: string;
  ariaLabel: string;
  className?: string;
}

export function AutoSaveInput({
  value,
  onSave,
  placeholder,
  ariaLabel,
  className,
}: AutoSaveInputProps) {
  const [draft, setDraft] = useState(value);
  const focused = useRef(false);

  useEffect(() => {
    if (!focused.current) setDraft(value);
  }, [value]);

  const commit = () => {
    const next = draft.trim();
    if (next.length > 0 && next !== value) onSave(next);
    else setDraft(value);
  };

  return (
    <Input
      value={draft}
      placeholder={placeholder}
      aria-label={ariaLabel}
      spellCheck={false}
      onFocus={() => {
        focused.current = true;
      }}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        focused.current = false;
        commit();
      }}
      onKeyDown={(e: KeyboardEvent<HTMLInputElement>) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          e.currentTarget.blur();
        } else if (e.key === 'Escape') {
          // 先还原再离开：取消的编辑不该顺带保存。
          e.preventDefault();
          setDraft(value);
          e.currentTarget.blur();
        }
      }}
      className={cn('h-8 text-xs', className)}
    />
  );
}

interface AutoSaveTextareaProps {
  /** `undefined` 与 `''` 都读作「没有描述」。 */
  value: string | undefined;
  /** 带着 trim 后的新值调用，含 `''`（清空）。 */
  onSave: (next: string) => void;
  placeholder?: string;
  ariaLabel: string;
  className?: string;
}

export function AutoSaveTextarea({
  value,
  onSave,
  placeholder,
  ariaLabel,
  className,
}: AutoSaveTextareaProps) {
  const [draft, setDraft] = useState(value ?? '');
  const focused = useRef(false);

  useEffect(() => {
    if (!focused.current) setDraft(value ?? '');
  }, [value]);

  const commit = () => {
    const next = draft.trim();
    if (next !== (value ?? '')) onSave(next);
    else setDraft(value ?? '');
  };

  return (
    <textarea
      value={draft}
      placeholder={placeholder}
      aria-label={ariaLabel}
      spellCheck={false}
      rows={3}
      onFocus={() => {
        focused.current = true;
      }}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        focused.current = false;
        commit();
      }}
      onKeyDown={(e: KeyboardEvent<HTMLTextAreaElement>) => {
        // 这里回车是换行 —— 描述是散文。Ctrl/⌘+Enter 才是提交，Esc 放弃本次编辑。
        if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
          e.preventDefault();
          e.currentTarget.blur();
        } else if (e.key === 'Escape') {
          e.preventDefault();
          setDraft(value ?? '');
          e.currentTarget.blur();
        }
      }}
      className={cn(
        'w-full resize-y rounded-md border border-input bg-background px-2 py-1.5 text-xs text-foreground',
        'placeholder:text-muted-foreground/70 focus:border-ring focus:outline-none',
        className,
      )}
    />
  );
}

/**
 * 基础信息列表 —— 行之间是发丝线，外面没有卡片。
 *
 * 这里做边界的正是分区标题，故行之间只需要发丝线，并且横跨分区的整个宽度 —— 与设置面板里
 * `SettingRow` 产生的列表同形，于是工作区自己的行和它上方的设置行读起来是同一种东西。
 */
export function InfoBlock({ children }: { children: React.ReactNode }) {
  return <div className="divide-y divide-border/60 border-y border-border/60">{children}</div>;
}

/**
 * `InfoBlock` 里带标签的一行。
 *
 * 标签与提示在左、控件在右 —— 就是 `SettingRow` 的布局，刻意如此，好让这两个字段不在
 * 设置面板已有的几种形态之外再引入第三种。提示也就因此待在自己的标签下面，而不是挂在
 * 控件末尾 —— 提示本就该在那儿。
 */
export function InfoField({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-start gap-6 py-3">
      <div className="w-40 shrink-0">
        <div className="text-xs font-medium text-foreground">{label}</div>
        {hint && <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">{hint}</p>}
      </div>
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}
