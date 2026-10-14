/**
 * MethodPicker —— 选 HTTP 方法的自绘 listbox。
 *
 * 替掉原生 `<select>`：原生控件无法把 `<option>` 子项与触发器分开上色，于是选中
 * 某个带色的方法（如 POST → 蓝）会把颜色渗到每个其他选项上。本组件把每个选项渲染
 * 成真正的 `<button>`，各自按 `methodColorVar` 上色。
 *
 * 触发器显示 **solid** 口味的 **MethodPill**，好让选中的动词读作控件而不是一段
 * 彩色文字；下拉行用同一枚药丸的 **tinted** 口味，免得比行内容更抢眼。
 *
 * 交互：
 *  - 点触发器 → 开 / 关
 *  - 点外部（mousedown）→ 关
 *  - Esc → 关
 *  - ↑ / ↓ → 移动 active 下标（不提交）
 *  - Enter → 提交 active 选项并关闭
 *  - Tab → 关（默认 tab 导航保留）
 *  - 点某个选项 → 提交并关闭
 */

import { useEffect, useRef, useState } from 'react';
import { ChevronDown } from 'lucide-react';
import { cn, focusRing } from '@/lib/utils';
import { HTTP_METHODS, type HttpMethod } from '@/types/domain';
import { MethodPill } from './method-pill';

export interface MethodPickerProps {
  value: HttpMethod;
  onChange: (m: HttpMethod) => void;
  className?: string;
}

export function MethodPicker({ value, onChange, className }: MethodPickerProps) {
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState<number>(() => {
    const i = HTTP_METHODS.indexOf(value);
    return i < 0 ? 0 : i;
  });

  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const optionRefs = useRef<(HTMLButtonElement | null)[]>([]);

  // 每次打开时把键盘游标重新锚到当前选中的方法上。会话中途的 ↑/↓ 移动不会被外部
  // value 变化打断（弹层开着时这类变化本应很少见）。
  useEffect(() => {
    if (!open) return;
    const i = HTTP_METHODS.indexOf(value);
    if (i >= 0 && i !== activeIndex) setActiveIndex(i);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, value]);

  // 打开期间焦点跟随 activeIndex。关闭时列表卸载，所以只在 `open` 为真时驱动焦点。
  useEffect(() => {
    if (!open) return;
    const el = optionRefs.current[activeIndex];
    el?.focus();
  }, [open, activeIndex]);

  // 点外部关闭。
  useEffect(() => {
    if (!open) return;
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (rootRef.current?.contains(target)) return;
      setOpen(false);
    };
    document.addEventListener('mousedown', onMouseDown);
    return () => document.removeEventListener('mousedown', onMouseDown);
  }, [open]);

  // 弹层打开期间的键盘处理。挂在 `document` 上，这样无论焦点在触发器还是在某个选项
  // 上（选项在打开时由上面的 effect 取得焦点），导航都生效。
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        setOpen(false);
        return;
      }
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setActiveIndex((i) => (i + 1) % HTTP_METHODS.length);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setActiveIndex((i) => (i - 1 + HTTP_METHODS.length) % HTTP_METHODS.length);
        return;
      }
      if (e.key === 'Enter') {
        e.preventDefault();
        const m = HTTP_METHODS[activeIndex];
        if (m) {
          onChange(m);
          setOpen(false);
        }
        return;
      }
      if (e.key === 'Tab') {
        // 按约定：不 preventDefault——让正常的 tab 导航照常走，只把弹层关掉。
        setOpen(false);
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [open, activeIndex, onChange]);

  const handleSelect = (m: HttpMethod) => {
    onChange(m);
    setOpen(false);
  };

  return (
    <div ref={rootRef} className={cn('relative inline-block', className)}>
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={cn(
          'flex h-9 items-center gap-1.5 rounded-md border border-input bg-background px-1.5 font-mono text-xs transition-colors',
          'hover:bg-accent/40',
          focusRing,
        )}
      >
        <MethodPill method={value} size="sm" variant="solid" />
        <ChevronDown
          className={cn(
            'h-3.5 w-3.5 text-muted-foreground transition-transform',
            open && 'rotate-180',
          )}
        />
      </button>
      {open && (
        <ul
          role="listbox"
          aria-label="HTTP method"
          className="absolute top-full left-0 z-50 mt-1 overflow-hidden rounded-md border border-border bg-popover py-1 shadow-lg"
        >
          {HTTP_METHODS.map((m, idx) => {
            const isSelected = m === value;
            const isActive = idx === activeIndex;
            return (
              <li key={m}>
                <button
                  ref={(el) => {
                    optionRefs.current[idx] = el;
                  }}
                  type="button"
                  role="option"
                  aria-selected={isSelected}
                  data-idx={idx}
                  onClick={() => handleSelect(m)}
                  onMouseEnter={() => setActiveIndex(idx)}
                  className={cn(
                    'flex w-full items-center gap-2 px-2 py-1.5 text-left text-xs outline-none transition-colors',
                    'hover:bg-accent',
                    'focus:bg-accent',
                    // 选中行与键盘 active 行用同一个 accent 底色，与触发器的控件感一致
                    (isActive || isSelected) && 'bg-accent',
                  )}
                >
                  <MethodPill
                    method={m}
                    size="sm"
                    // 选中行用 solid（与触发器一致）；其余行保持 tinted，免得下拉比它伴随的
                    // 行内容更抢眼
                    variant={isSelected ? 'solid' : 'tinted'}
                  />
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
