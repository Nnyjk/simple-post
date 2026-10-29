/**
 * BaseUrlPicker —— HttpTester 里位于方法选择器与 URL 输入框之间的下拉。
 *
 * 它只有两档：**跟随模块** 与 **自定义**。地址住在环境里的「模块 × 环境」那一格，
 * 故切环境即换地址，而接口本身的设置不动。
 *
 * 组件完全受控，由父级持有：
 *   - `selection` —— 当前是哪一档
 *
 * 「自定义」刻意**没有自己的输入框**：选中它就把完整地址交给 URL 栏，在这里再加
 * 一个 host 框会为同一个请求制造第二个真相来源。带 host 的自定义仍按该 host 前缀
 * 解析 —— 此时 `resolvedBaseUrl` 非空 —— 并显示在触发器的副标签上。
 *
 * 父级同时传入已解析的地址供触发器标签使用；此处不算，以保持选择器无知。
 *
 * 视觉风格：卡片式触发器（rounded-lg + 强调色图标容器），下移行采用 SettingRow 风格
 * （图标容器 + 主标签 + 副标签 + 末尾徽标），与新的设置分类观感一致。
 *
 * 位置：本组件住在 `features/http-tester/` 而非 `components/ui/`，因为它不是基础件 ——
 * 它写死了「跟随模块 / 自定义」的语义，且只有一个消费方（HttpTester）。它组合的那几个
 * 基础件才是 `components/ui/` 里货真价实的 primitive。
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import { Check, ChevronDown, Globe, Pencil } from 'lucide-react';
import { cn, focusRing } from '@/lib/utils';
import { IconTile } from '@/components/ui/icon-tile';
import type { HueConcept } from '@/lib/hue';

/**
 * 触发器此刻显示什么。`module` 是「跟随模块」—— 地址取环境里本模块那一格；
 * `override` 是按接口的自定义地址。
 */
export type BaseUrlSelection = { kind: 'module' } | { kind: 'override' };

export interface BaseUrlPickerProps {
  /**
   * 当前已解析的模块地址。只用于触发器的副标签（如「· api.demo.dev」），让用户一眼
   * 确认实际 host。
   */
  resolvedBaseUrl: string;
  selection: BaseUrlSelection;
  onSelectionChange: (next: BaseUrlSelection) => void;
  className?: string;
}

const TRIGGER_WIDTH = 'w-52';
const POPOVER_WIDTH = 'w-80';

export function BaseUrlPicker({
  resolvedBaseUrl,
  selection,
  onSelectionChange,
  className,
}: BaseUrlPickerProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

  // 点击外部关闭 —— 用 mousedown，好让落在某一行上的点击先被那一行捕获。
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

  // Esc 关闭。
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        setOpen(false);
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open]);

  const triggerLabel = selection.kind === 'override' ? '自定义' : '跟随模块';

  // 副标签回答「实际会用哪个 host？」。自定义模式下可能根本没有 host —— URL 栏承载
  // 完整地址 —— 故明说这一点，而不是渲染一行空的。
  const triggerSubLabel = useMemo(() => {
    if (!resolvedBaseUrl) {
      return selection.kind === 'override' ? '· URL 栏直填完整地址' : '';
    }
    return `· ${hostOf(resolvedBaseUrl)}`;
  }, [selection, resolvedBaseUrl]);

  // 图标容器与「选中」提示所用的强调色。baseUrl 自有色相，不会再与它曾共用的集合色
  // 混淆。「自定义」行用 `variable` —— 一个由用户提供的值 —— 而不是借 `module` 的琥珀色，
  // 后者会把树的概念拖进选择器。
  return (
    <div ref={rootRef} className={cn('relative inline-block', className)}>
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        data-testid="baseurl-trigger"
        title={resolvedBaseUrl || '点击选择地址来源'}
        className={cn(
          // 卡片式触发器 —— 与 SettingsRow 的描边一致。嵌一个图标容器，使这个便利性读起来
          // 是「这是 baseUrl 选择器」而非「这是一个带箭头的下拉」。
          'flex h-9 items-center gap-2 rounded-lg border border-input bg-background px-2 text-xs transition-colors',
          'hover:bg-accent/40',
          TRIGGER_WIDTH,
          !resolvedBaseUrl && selection.kind === 'module' && 'text-muted-foreground',
          focusRing,
        )}
      >
        <IconTile size="xs" bordered concept="baseurl" icon={<Globe className="h-3 w-3" />} />
        <span className="min-w-0 flex-1 text-left leading-tight">
          <span className="block truncate font-medium">{triggerLabel}</span>
          {triggerSubLabel && (
            <span className="block truncate font-mono text-[10px] text-muted-foreground/70 leading-tight">
              {triggerSubLabel}
            </span>
          )}
        </span>
        <ChevronDown
          className={cn(
            'h-3.5 w-3.5 shrink-0 text-muted-foreground transition-transform',
            open && 'rotate-180',
          )}
        />
      </button>

      {open && (
        <div
          role="listbox"
          aria-label="选择地址来源"
          className={cn(
            'absolute top-full left-0 z-50 mt-1 overflow-hidden rounded-lg border border-border bg-popover shadow-lg',
            POPOVER_WIDTH,
          )}
        >
          <ul className="max-h-80 overflow-auto p-1">
            <Row
              selected={selection.kind === 'module'}
              onSelect={() => {
                onSelectionChange({ kind: 'module' });
                setOpen(false);
              }}
              concept="baseurl"
              icon={<Globe className="h-3.5 w-3.5" />}
              primary="跟随模块"
              secondary={resolvedBaseUrl ? resolvedBaseUrl : '环境里本模块没有配地址'}
            />
            <Row
              selected={selection.kind === 'override'}
              onSelect={() => {
                onSelectionChange({ kind: 'override' });
                // 关掉：这一行的全部职责就是把 URL 栏交给用户，而用户马上就要往里打字 ——
                // 把 popover 留在 URL 栏上方只会让整个动作显得没反应。
                setOpen(false);
              }}
              concept="variable"
              icon={<Pencil className="h-3.5 w-3.5" />}
              primary="自定义"
              secondary="URL 栏直接填完整地址，不拼接模块地址"
            />
          </ul>
        </div>
      )}
    </div>
  );
}

interface RowProps {
  selected: boolean;
  onSelect: () => void;
  /** 本行图标块的语义色相。 */
  concept: HueConcept;
  icon: React.ReactNode;
  primary: string;
  secondary?: string;
}

function Row({ selected, onSelect, concept, icon, primary, secondary }: RowProps) {
  return (
    <li>
      <button
        type="button"
        role="option"
        aria-selected={selected}
        onClick={onSelect}
        className={cn(
          'flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-xs transition-colors',
          'hover:bg-accent hover:text-foreground',
          selected && 'bg-accent text-foreground',
        )}
      >
        <IconTile size="md" bordered concept={concept} icon={icon} />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-xs font-medium leading-tight">{primary}</span>
          {secondary && (
            <span className="block truncate font-mono text-[10px] text-muted-foreground/80 leading-tight">
              {secondary}
            </span>
          )}
        </span>
        {selected && <Check className="h-3.5 w-3.5 shrink-0 text-primary" aria-hidden />}
      </button>
    </li>
  );
}

function hostOf(url: string): string {
  if (!url) return '';
  // 去掉 scheme 与 path，只留 host[:port] 部分用于显示。
  const m = url.match(/^[a-z][a-z0-9+.-]*:\/\/([^/?#]+)/i);
  if (m) return m[1];
  return url.length > 28 ? `${url.slice(0, 28)}…` : url;
}
