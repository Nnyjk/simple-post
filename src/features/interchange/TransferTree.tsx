import { useEffect, useRef, useState } from 'react';
import { ChevronRight } from 'lucide-react';

import type { TransferEndpoint, TransferNode } from '@/lib/interchange-types';
import type { HttpMethod } from '@/types/domain';
import { cn, focusRingBare } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';
import { MethodPill } from '@/components/ui/method-pill';
import { Tooltip } from '@/components/ui/tooltip';
import { groupState, isIndeterminate } from './selection';

/**
 * 导入侧会带来 `HttpMethod::Custom` 的冷门动词（`TRACE` 落库后就是这个形状），
 * 而 TS 侧的 `HttpMethod` 是封闭联合。这里不做校验，只把字符串交给展示层——
 * 校验动词合法性的地方在解析器与导出器，不在渲染这 80 行里。
 */
const asMethod = (raw: string): HttpMethod => raw as HttpMethod;

interface CheckBoxProps {
  checked: boolean;
  indeterminate?: boolean;
  disabled?: boolean;
  label: string;
  onChange: () => void;
}

/**
 * 勾选框。三态里的「半选」没有对应的 `checked` 值，只能设 DOM 属性——
 * 故这里用 `useEffect` 同步，而不是在 JSX 上写一个不存在的属性。
 */
function CheckBox({ checked, indeterminate, disabled, label, onChange }: CheckBoxProps) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = !!indeterminate && !checked;
  }, [indeterminate, checked]);
  return (
    <input
      ref={ref}
      type="checkbox"
      checked={checked}
      disabled={disabled}
      aria-label={label}
      onChange={onChange}
      onClick={(e) => e.stopPropagation()}
      className={cn(
        'h-3.5 w-3.5 shrink-0 cursor-pointer appearance-none rounded-[3px] border border-muted-foreground/60 bg-transparent transition-colors',
        'checked:border-primary checked:bg-primary',
        'disabled:cursor-not-allowed disabled:opacity-40',
        focusRingBare,
      )}
    />
  );
}

interface RowProps {
  selected: ReadonlySet<string>;
  onToggleEndpoint: (key: string) => void;
  onToggleGroup: (node: TransferNode) => void;
}

function GroupRow({
  node,
  depth,
  selected,
  onToggleGroup,
  children,
}: RowProps & {
  node: TransferNode;
  depth: number;
  children?: React.ReactNode;
}) {
  // 默认全展开：用户是来挑东西的，一层层点开只会挡住内容。集合通常不超过十来个。
  const [open, setOpen] = useState(true);
  const state = groupState(node, selected);
  const isEmpty = node.endpointTotal === 0;

  return (
    <>
      {/* 分组行本身可点：点空白处等于点复选框。这一行没有别的动作，
          而「选不选这一组」正是它唯一要表达的事。 */}
      <div
        role="row"
        className={cn(
          'group flex h-7 cursor-pointer items-center gap-2 rounded-sm pr-2 transition-colors hover:bg-overlay-hover',
          depth === 0 && 'mt-1 first:mt-0',
        )}
        style={{ paddingLeft: depth * 14 + 8 }}
        onClick={() => {
          if (!isEmpty) onToggleGroup(node);
        }}
      >
        <span
          role="button"
          tabIndex={0}
          aria-label={open ? `折叠 ${node.name}` : `展开 ${node.name}`}
          onClick={(e) => {
            e.stopPropagation();
            setOpen(!open);
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault();
              e.stopPropagation();
              setOpen(!open);
            }
          }}
          className={cn(
            'flex h-4 w-4 shrink-0 items-center justify-center rounded-sm text-faint hover:text-muted-foreground',
            focusRingBare,
          )}
        >
          <ChevronRight className={cn('h-3 w-3 transition-transform', open && 'rotate-90')} />
        </span>
        <CheckBox
          checked={state === 'all'}
          indeterminate={isIndeterminate(node, selected)}
          disabled={isEmpty}
          label={`选择分组 ${node.name}`}
          onChange={() => onToggleGroup(node)}
        />
        <span className="min-w-0 flex-1 truncate text-xs font-medium text-foreground">
          {node.name}
        </span>
        <span className="shrink-0 text-[10px] tabular-nums text-faint">
          {node.endpointTotal} 个接口
        </span>
      </div>
      {open && children}
    </>
  );
}

function EndpointRow({
  endpoint,
  depth,
  selected,
  onToggleEndpoint,
}: RowProps & { endpoint: TransferEndpoint; depth: number }) {
  return (
    <div
      role="row"
      className="flex h-7 cursor-pointer items-center gap-2 rounded-sm pr-2 transition-colors hover:bg-overlay-hover"
      style={{ paddingLeft: depth * 14 + 8 }}
      onClick={() => onToggleEndpoint(endpoint.key)}
    >
      {/* 只为了让正文对齐到分组名列，与其它地方的缩进同一条理由。 */}
      <span className="w-4 shrink-0" />
      <CheckBox
        checked={selected.has(endpoint.key)}
        label={`选择接口 ${endpoint.name}`}
        onChange={() => onToggleEndpoint(endpoint.key)}
      />
      <MethodPill
        method={asMethod(endpoint.method)}
        size="xs"
        label={endpoint.method.charAt(0)}
        title={endpoint.method}
      />
      <span className="min-w-0 flex-1 truncate text-xs text-foreground">{endpoint.name}</span>
      {endpoint.duplicate && (
        <Tooltip content="项目里已经有同一个接口，落库时会按指纹跳过">
          <Badge variant="muted">已存在</Badge>
        </Tooltip>
      )}
      <span className="w-40 shrink-0 truncate text-right font-mono text-[11px] text-faint">
        {endpoint.path}
      </span>
    </div>
  );
}

function Node({ node, depth, ...rest }: RowProps & { node: TransferNode; depth: number }) {
  return (
    <GroupRow node={node} depth={depth} {...rest}>
      {node.endpoints.map((e) => (
        <EndpointRow key={e.key} endpoint={e} depth={depth + 1} {...rest} />
      ))}
      {node.children.map((c) => (
        <Node key={c.key} node={c} depth={depth + 1} {...rest} />
      ))}
    </GroupRow>
  );
}

interface TransferTreeProps extends RowProps {
  groups: TransferNode[];
}

/**
 * 勾选树。**导入与导出共用这一个组件**：两边的线类型是同一个 `TransferNode`，
 * 差别（重复标记、环境提示）在各自的行上显示，不在这里分叉。
 *
 * 分组之间靠**分隔线**而不是套一层边框——外层已经是弹窗面板，再加边框会读成卡片套卡片。
 */
export function TransferTree({
  groups,
  selected,
  onToggleEndpoint,
  onToggleGroup,
}: TransferTreeProps) {
  if (groups.length === 0) {
    return (
      <p className="px-2 py-6 text-center text-[11px] text-muted-foreground">
        这份文件里没有任何可导入的请求。
      </p>
    );
  }
  return (
    <div role="tree" aria-label="可勾选的条目" className="py-1">
      {groups.map((g) => (
        <Node
          key={g.key}
          node={g}
          depth={0}
          selected={selected}
          onToggleEndpoint={onToggleEndpoint}
          onToggleGroup={onToggleGroup}
        />
      ))}
    </div>
  );
}
