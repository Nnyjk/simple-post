/**
 * 预览响应的可点选 JSON 树。
 *
 * 存在的理由：动态变量的抽取规则是一条要手写的 JSONPath，而用户往往并不知道接口到底返回
 * 什么形状。于是预览把响应的原始 JSON 摊成一棵树，点哪个节点就把它的 JSONPath 交出去。
 *
 * 展开状态自持在每个节点里（第一次渲染时只有根展开）：树是预览的一次性产物，父组件不该
 * 为了记住「哪个分支开着」多存一份状态；换一份响应时它会整个重挂，展开状态自然归零。
 *
 * 一次最多画 200 个孩子 —— 预览是给人瞄一眼挑路径的，不是导出器。超出的部分明说还有多少，
 * 不静默截断成「看起来就这么点」。
 */
import { useState } from 'react';
import { ChevronRight } from 'lucide-react';
import { cn, focusRing } from '@/lib/utils';
import { pathForIndex, pathForKey } from './extract-path';

export interface PreviewJsonTreeProps {
  /** 要展示的 JSON 值（通常是预览返回的 `bodyJson`）。 */
  value: unknown;
  /** 这个值自己的 JSONPath。根传 `$`。 */
  path: string;
  /** 点某个节点：交出它的路径与该节点的值，由面板写进抽取规则、生成脚本片段。 */
  onPick: (path: string, value: unknown) => void;
}

/** 单个容器一次最多渲染的孩子数。 */
const MAX_CHILDREN = 200;
/** 字符串值的预览长度（字符）。 */
const MAX_STRING = 80;

export function PreviewJsonTree({ value, path, onPick }: PreviewJsonTreeProps) {
  return (
    <div role="tree" aria-label="响应 JSON" className="font-mono text-xs leading-relaxed">
      <TreeNode value={value} path={path} depth={0} onPick={onPick} />
    </div>
  );
}

type JsonKind = 'object' | 'array' | 'string' | 'number' | 'boolean' | 'null' | 'other';

function kindOf(value: unknown): JsonKind {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'array';
  const type = typeof value;
  if (type === 'object' || type === 'string' || type === 'number' || type === 'boolean') {
    return type;
  }
  return 'other';
}

interface ChildNode {
  /** 行首显示的键名（对象）或下标（数组）。 */
  label: string;
  path: string;
  value: unknown;
}

function childNodes(value: unknown, path: string): ChildNode[] {
  if (Array.isArray(value)) {
    return value.map((item, index) => ({
      label: `[${index}]`,
      path: pathForIndex(path, index),
      value: item,
    }));
  }
  if (kindOf(value) === 'object') {
    return Object.entries(value as Record<string, unknown>).map(([key, item]) => ({
      label: key,
      path: pathForKey(path, key),
      value: item,
    }));
  }
  return [];
}

/** 一行右半边那句值概要：容器说有多少项，标量给截断后的字面量。 */
function summarize(value: unknown, kind: JsonKind): string {
  switch (kind) {
    case 'array':
      return `[ … ] ${(value as unknown[]).length} 项`;
    case 'object':
      return `{ … } ${Object.keys(value as Record<string, unknown>).length} 项`;
    case 'string': {
      const text = value as string;
      return text.length > MAX_STRING
        ? `${JSON.stringify(text.slice(0, MAX_STRING))}…`
        : JSON.stringify(text);
    }
    case 'null':
      return 'null';
    case 'other':
      return String(value);
    default:
      return String(value);
  }
}

function TreeNode({
  value,
  path,
  depth,
  label,
  onPick,
}: {
  value: unknown;
  path: string;
  depth: number;
  label?: string;
  onPick: PreviewJsonTreeProps['onPick'];
}) {
  const kind = kindOf(value);
  const expandable = kind === 'object' || kind === 'array';
  // 只有根默认展开：一棵长响应的所有分支都开着，等于没树。
  const [open, setOpen] = useState(depth === 0);
  const entries = expandable && open ? childNodes(value, path) : [];
  const children = entries.slice(0, MAX_CHILDREN);
  const hidden = entries.length - children.length;

  return (
    <div role="treeitem" aria-expanded={expandable ? open : undefined} aria-level={depth + 1}>
      <div
        className="group flex items-start gap-1 rounded pr-1 hover:bg-overlay-hover"
        style={{ paddingLeft: depth * 12 + 4 }}
      >
        {expandable ? (
          <button
            type="button"
            aria-expanded={open}
            aria-label={open ? '收起' : '展开'}
            onClick={() => setOpen((previous) => !previous)}
            className={cn(
              'mt-0.5 shrink-0 rounded text-muted-foreground hover:text-foreground',
              focusRing,
            )}
          >
            <ChevronRight className={cn('h-3 w-3 transition-transform', open && 'rotate-90')} />
          </button>
        ) : (
          <span className="mt-0.5 h-3 w-3 shrink-0" aria-hidden />
        )}
        <button
          type="button"
          onClick={() => onPick(path, value)}
          title={`把这条路径写进抽取规则：${path}`}
          className={cn(
            'flex min-w-0 flex-1 items-baseline gap-1.5 rounded py-0.5 text-left',
            focusRing,
          )}
        >
          {label !== undefined && <span className="shrink-0 text-foreground">{label}:</span>}
          <span className="truncate text-muted-foreground">{summarize(value, kind)}</span>
          <span className="ml-auto shrink-0 text-nano text-faint opacity-0 group-hover:opacity-100 group-focus-within:opacity-100">
            用这条
          </span>
        </button>
      </div>
      {expandable && open && (
        <>
          {children.map((child) => (
            <TreeNode
              key={child.path}
              value={child.value}
              path={child.path}
              depth={depth + 1}
              label={child.label}
              onPick={onPick}
            />
          ))}
          {hidden > 0 && (
            <div className="text-nano text-faint" style={{ paddingLeft: (depth + 1) * 12 + 4 }}>
              还有 {hidden} 项未显示（预览只画前 {MAX_CHILDREN} 项）
            </div>
          )}
        </>
      )}
    </div>
  );
}
