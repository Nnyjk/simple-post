/**
 * EnvVarPicker —— 列出当前环境变量、供 URL 输入框快速插入 `{{key}}` 的小浮层。
 *
 * 纯展示的受控组件，父组件掌管：
 *  - 要显示的 `entries`（通常已按用户输入的半个变量名过滤）
 *  - 选中行下标及其变更方式（`onSelectedChange`）
 *  - 打开 / 关闭状态
 *
 * 位置锚在 `anchorRef`：落在 `top: 输入框底 + 4px`、`left: 输入框左`，宽度 >= 240px。
 */

import { useEffect, useRef } from 'react';
import { ChevronRight } from 'lucide-react';
import { cn } from '@/lib/utils';

export interface VarEntry {
  key: string;
  value: string;
  /**
   * 这一项来自哪里。环境变量没有这个字段（它们是默认的那一半）。
   *
   * 动态变量与环境变量并进同一份候选（不新开页签），故用户要能一眼分出哪个是自动取回
   * 的、哪个是自己填的。
   */
  origin?: 'environment' | 'dynamic';
  /** 动态变量的取值来源与缓存年龄，供悬停时显示。 */
  hint?: string;
}

interface EnvVarPickerProps {
  open: boolean;
  onClose: () => void;
  entries: VarEntry[];
  selected: number;
  onSelectedChange: (next: number) => void;
  onSelect: (key: string) => void;
  /** 定位浮层的锚元素（URL 输入框）。 */
  anchorRef: React.RefObject<HTMLElement | null>;
  /** 头部标题，可选，如当前环境名。 */
  envName?: string;
}

function truncate(s: string | undefined, n: number): string {
  const str = s ?? '';
  return str.length <= n ? str : `${str.slice(0, n - 1)}…`;
}

export function EnvVarPicker({
  open,
  onClose,
  entries,
  selected,
  onSelectedChange,
  onSelect,
  anchorRef,
  envName,
}: EnvVarPickerProps) {
  const popRef = useRef<HTMLDivElement>(null);

  // 打开、窗口滚动或缩放时都重新定位。
  useEffect(() => {
    if (!open) return;
    const setPos = () => {
      const el = popRef.current;
      const anchor = anchorRef.current;
      if (!el || !anchor) return;
      const rect = anchor.getBoundingClientRect();
      el.style.left = `${rect.left}px`;
      el.style.top = `${rect.bottom + 4}px`;
      el.style.minWidth = `${Math.max(240, rect.width)}px`;
    };
    setPos();
    window.addEventListener('resize', setPos);
    window.addEventListener('scroll', setPos, true);
    return () => {
      window.removeEventListener('resize', setPos);
      window.removeEventListener('scroll', setPos, true);
    };
  }, [open, anchorRef, entries.length]);

  // 点外部关闭，但点在 URL 输入框内不算。
  useEffect(() => {
    if (!open) return;
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (popRef.current?.contains(target)) return;
      if (anchorRef.current?.contains(target)) return;
      onClose();
    };
    document.addEventListener('mousedown', onMouseDown);
    return () => document.removeEventListener('mousedown', onMouseDown);
  }, [open, onClose, anchorRef]);

  // 条目变少时把选中项收回范围。
  useEffect(() => {
    if (entries.length === 0) return;
    if (selected < 0 || selected >= entries.length) onSelectedChange(0);
  }, [entries.length, selected, onSelectedChange]);

  if (!open) return null;

  return (
    <div
      ref={popRef}
      role="listbox"
      aria-label="变量"
      data-testid="env-var-picker"
      className="fixed z-50 max-h-64 overflow-y-auto rounded-md border border-border bg-popover text-popover-foreground shadow-lg animate-fade-in"
      onMouseDown={(e) => e.preventDefault() /* 保持输入框聚焦 */}
    >
      <div className="flex items-center gap-1.5 border-b border-border px-2 py-1 label-caps">
        <ChevronRight className="h-3 w-3" />
        插入变量{envName ? ` · ${envName}` : ''}
      </div>
      {entries.length === 0 && (
        <div className="px-3 py-6 text-center text-xs text-muted-foreground">没有匹配的变量</div>
      )}
      {entries.map((e, idx) => {
        const active = idx === selected;
        return (
          <button
            key={e.key}
            type="button"
            role="option"
            aria-selected={active}
            data-idx={idx}
            onMouseEnter={() => onSelectedChange(idx)}
            onClick={() => onSelect(e.key)}
            className={cn(
              'flex w-full items-center gap-2 px-2 py-1.5 text-left text-xs',
              active ? 'bg-primary/15 text-foreground' : 'hover:bg-accent/60',
            )}
          >
            <span className="shrink-0 font-mono text-primary">{`{{${e.key}}}`}</span>
            <span className="min-w-0 flex-1 truncate text-muted-foreground">
              {truncate(e.value, 60)}
            </span>
            {e.hint && (
              <span className="shrink-0 text-[10px] text-muted-foreground/80">{e.hint}</span>
            )}
          </button>
        );
      })}
      <div className="flex items-center gap-3 border-t border-border bg-card/40 px-2 py-1 text-[10px] text-muted-foreground">
        <span>↑↓ 选择</span>
        <span>Tab/Enter 插入</span>
        <span>Esc 关闭</span>
      </div>
    </div>
  );
}

/** 给 HttpTester 用：从当前草稿 URL 里取出用户正在输入的半个变量名，即最后一个 `{{` 之后、
 *  任何 `}}` 之前的文本。不在未闭合的 `{{…}}` 块里则返回 `''`。 */
export function extractPartialVar(url: string): string {
  const lastOpen = url.lastIndexOf('{{');
  if (lastOpen < 0) return '';
  const tail = url.slice(lastOpen + 2);
  if (tail.includes('}}')) return '';
  const match = tail.match(/^([^{}\s]*)/);
  return match ? match[1] : '';
}

/** 给 HttpTester 用：用户是否正处在一个未闭合的 `{{…}}` 块里，即浮层该不该可见。 */
export function shouldShowVarPicker(url: string): boolean {
  const lastOpen = url.lastIndexOf('{{');
  if (lastOpen < 0) return false;
  const tail = url.slice(lastOpen + 2);
  return !tail.includes('}}');
}
