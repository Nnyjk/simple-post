/**
 * BaseUrlPicker —— HttpTester 里位于方法选择器与 URL 输入框之间的下拉。
 *
 * 它驱动的是一个 baseUrl *服务*（像「鉴权服务」/「数据服务」这样的命名槽位）加一个可选的
 * 按接口 override。实际 URL 在解析时从 `env.moduleBaseUrls[defId][moduleId]` 查得
 * （见 lib/url.ts），故切换当前环境会自动换 URL，而不必改动接口的选择。
 *
 * `definitions` 是项目的完整服务列表 —— 服务属于项目，每个模块都看得见全部，故这里每一个
 * 都是真实可选项。`defaultDefinitionId` 因此是项目级的 ★
 * （`Project.defaultBaseUrlDefinitionId`），不是某个模块的。
 *
 * 组件完全受控，由父级持有：
 *   - `selection` —— 当前激活的是哪个命名槽位（或「默认」/「override」）
 *
 * 列表里只有项目的服务与 自定义，别无其他。「跟随项目默认」就是带 默认 徽标的那一行
 * 本身（见下方 map 里的 `isProjectDefault` 分支）。
 *
 * 自定义 (override) 刻意**没有自己的输入框**：选中它就把完整地址交给 URL 栏，在这里再加
 * 一个 host 框会为同一个请求制造第二个真相来源。带 host 的 override 仍按该 host 前缀解析
 * —— 此时 `resolvedBaseUrl` 非空 —— 并显示在触发器的副标签上。
 *
 * 父级同时传入已解析的 baseUrl 供触发器标签使用；此处不算，以保持选择器无知。
 *
 * 视觉风格：卡片式触发器（rounded-lg + 强调色图标容器），下移行采用 SettingRow 风格
 * （图标容器 + 主标签 + 副标签 + 末尾徽标），与新的设置分类观感一致。
 *
 * 位置：本组件住在 `features/http-tester/` 而非 `components/ui/`，因为它不是基础件 ——
 * 它导入 `BaseUrlDefinition` 领域类型、写死了「哪个命名槽位激活」的语义，且只有一个
 * 消费方（HttpTester）。它组合的那三个基础件才是 `components/ui/` 里货真价实的 primitive。
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import { Check, ChevronDown, Globe, Pencil } from 'lucide-react';
import { cn, focusRing } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';
import { IconTile } from '@/components/ui/icon-tile';
import type { HueConcept } from '@/lib/hue';
import type { BaseUrlDefinition } from '@/types/domain';

/**
 * 触发器此刻显示什么。`default` 变体表示「跟随项目默认定义」；`definition` 是显式
 * 指定的命名槽位；`override` 是按接口的自定义 URL。
 */
export type BaseUrlSelection =
  { kind: 'default' } | { kind: 'definition'; definitionId: string } | { kind: 'override' };

export interface BaseUrlPickerProps {
  /**
   * 项目的服务列表 —— 驱动下拉的各行。不按接口所属模块过滤：列表是项目级的，每个模块
   * 的接口都可以指向其中任意一个。
   */
  definitions: readonly BaseUrlDefinition[];
  /** 项目级默认服务 id（项目没有默认时可为 null）。 */
  defaultDefinitionId: string | null;
  /**
   * 当前已解析的 baseUrl。只用于触发器的副标签（如「· api.demo.dev」），让用户一眼
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
  definitions,
  defaultDefinitionId,
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

  // 默认行的标签：项目 ★ 有名字时用它的名字，否则显式写「无」，让空态明显而不是读起来
  // 像某个名字。自定义行始终显示「自定义」。
  const defaultDef = useMemo(
    () => definitions.find((d) => d.id === defaultDefinitionId) ?? null,
    [definitions, defaultDefinitionId],
  );
  const selectedDef = useMemo(() => {
    if (selection.kind !== 'definition') return null;
    return definitions.find((d) => d.id === selection.definitionId) ?? null;
  }, [selection, definitions]);

  const triggerLabel = useMemo(() => {
    if (selection.kind === 'definition' && selectedDef) return selectedDef.name;
    if (selection.kind === 'override') return '自定义';
    // 「跟随项目默认」读起来就该是*那个服务*，而不是一个单独的「项目默认」选项：列表里
    //   没有这一行，触发器必须点名列表所高亮的同一个东西。
    return defaultDef ? defaultDef.name : '项目无默认服务';
  }, [selection, selectedDef, defaultDef]);

  // 副标签回答「实际会用哪个 host？」。自定义 模式下可能根本没有 host —— URL 栏承载
  // 完整地址 —— 故明说这一点，而不是渲染一行空的。
  const triggerSubLabel = useMemo(() => {
    if (selection.kind === 'override') {
      return resolvedBaseUrl ? `· ${hostOf(resolvedBaseUrl)}` : '· URL 栏直填完整地址';
    }
    if (!resolvedBaseUrl) return '';
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
        title={resolvedBaseUrl || '点击选择 baseUrl 来源'}
        className={cn(
          // 卡片式触发器 —— 与 SettingsRow 的描边一致。嵌一个图标容器，使这个便利性读起来
          // 是「这是 baseUrl 选择器」而非「这是一个带箭头的下拉」。
          'flex h-9 items-center gap-2 rounded-lg border border-input bg-background px-2 text-xs transition-colors',
          'hover:bg-accent/40',
          TRIGGER_WIDTH,
          !defaultDef && selection.kind === 'default' && 'text-muted-foreground',
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
          aria-label="选择 baseUrl 来源"
          className={cn(
            'absolute top-full left-0 z-50 mt-1 overflow-hidden rounded-lg border border-border bg-popover shadow-lg',
            POPOVER_WIDTH,
          )}
        >
          <ul className="max-h-80 overflow-auto p-1">
            {definitions.map((def) => {
              // 带 默认 徽标的那一行**就是**「跟随项目默认」这个选项：选它不存任何 definition
              // id，故接口会继续跟随项目此后加星的那个服务。
              const isProjectDefault = def.id === defaultDefinitionId;
              return (
                <Row
                  key={def.id}
                  selected={
                    selection.kind === 'default'
                      ? isProjectDefault
                      : selection.kind === 'definition' && selection.definitionId === def.id
                  }
                  onSelect={() => {
                    onSelectionChange(
                      isProjectDefault
                        ? { kind: 'default' }
                        : { kind: 'definition', definitionId: def.id },
                    );
                    setOpen(false);
                  }}
                  concept="baseurl"
                  icon={<Globe className="h-3.5 w-3.5" />}
                  primary={def.name}
                  secondary={def.description}
                  selectedHint={isProjectDefault ? '默认' : null}
                />
              );
            })}
            {selection.kind === 'default' && !defaultDef && (
              // 仅当项目根本没有带星服务时才走到这里：接口在跟随默认，却没有默认可跟随。
              // 没有这句，列表会打开成什么都没选中且毫无解释。
              <li className="px-2 py-1.5 text-[11px] text-muted-foreground">
                该项目尚未设置默认服务
              </li>
            )}
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
              secondary="URL 栏直接填完整地址，不拼接环境前缀"
              selectedHint={null}
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
  /** 显示在解析为项目默认那一行上的「默认」徽标。 */
  selectedHint?: string | null;
}

function Row({ selected, onSelect, concept, icon, primary, secondary, selectedHint }: RowProps) {
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
        {selectedHint && <Badge variant="success">{selectedHint}</Badge>}
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
