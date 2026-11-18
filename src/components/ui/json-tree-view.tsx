import { useEffect, useMemo, useState, type MouseEvent } from 'react';
import { Braces, ChevronRight, Copy, KeyRound, Route } from 'lucide-react';
import { cn, focusRingBare } from '@/lib/utils';
import {
  defaultExpandedPaths,
  flattenTree,
  pairText,
  parseJsonTree,
  valueText,
  type JsonTreeNode,
  type JsonValue,
} from '@/lib/json-tree';
import { ContextMenu, type ContextMenuItem } from './context-menu';

export interface JsonTreeViewProps {
  /** JSON 文本。解析失败时本组件不渲染任何东西，由调用方退回原文并给提示。 */
  text: string;
  /** 长字符串值是否在内容宽度内折行；默认 false，靠横向滚动看完整内容。 */
  wrap?: boolean;
}

/** 标量按类型取色：字符串暖绿、数字橙、布尔与 null 蓝，与编辑器的 JSON 高亮同一族。 */
function scalarClass(value: JsonValue): string {
  if (typeof value === 'string') return 'text-success-foreground';
  if (typeof value === 'number') return 'text-warning-foreground';
  return 'text-primary';
}

/**
 * 可折叠的 JSON 键值树，带逐行右键菜单。
 *
 * 响应体与请求体两侧共用它：它只认文本与换行开关，不读 store、不依赖任何 feature。
 * 右键条目的复制目标都是那一行的节点，故菜单不需要知道树的形状。
 */
export function JsonTreeView({ text, wrap = false }: JsonTreeViewProps) {
  const root = useMemo(() => {
    const result = parseJsonTree(text);
    return result.ok ? result.root : null;
  }, [text]);

  const [expanded, setExpanded] = useState<Set<string>>(
    () => new Set(root ? defaultExpandedPaths(root) : []),
  );
  // 文本换了一棵树：展开状态交回默认值，旧路径的痕迹不会留到新树上。
  useEffect(() => {
    setExpanded(new Set(root ? defaultExpandedPaths(root) : []));
  }, [root]);

  const [menu, setMenu] = useState<{ node: JsonTreeNode; x: number; y: number } | null>(null);

  const rows = useMemo(
    () => (root ? flattenTree(root, (node) => expanded.has(node.path)) : []),
    [root, expanded],
  );

  const toggle = (path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const copy = (value: string) => {
    void navigator.clipboard?.writeText(value);
  };

  const items: ContextMenuItem[] = menu
    ? [
        {
          kind: 'item',
          label: '复制键名',
          icon: <KeyRound className="h-3.5 w-3.5" />,
          // 数组元素与根节点没有键名，复制键名对它们没有意义。
          disabled: menu.node.key === null,
          onClick: () => copy(menu.node.key ?? ''),
        },
        {
          kind: 'item',
          label: '复制值',
          icon: <Copy className="h-3.5 w-3.5" />,
          onClick: () => copy(valueText(menu.node)),
        },
        {
          kind: 'item',
          label: '复制键值对',
          icon: <Copy className="h-3.5 w-3.5" />,
          onClick: () => copy(pairText(menu.node)),
        },
        { kind: 'divider' },
        {
          kind: 'item',
          label: '复制子树 JSON',
          icon: <Braces className="h-3.5 w-3.5" />,
          onClick: () => copy(menu.node.json),
        },
        {
          kind: 'item',
          label: '复制 JSON 路径',
          icon: <Route className="h-3.5 w-3.5" />,
          onClick: () => copy(menu.node.path),
        },
      ]
    : [];

  const openMenu = (event: MouseEvent, node: JsonTreeNode) => {
    event.preventDefault();
    setMenu({ node, x: event.clientX, y: event.clientY });
  };

  if (!root) return null;

  return (
    <div
      className={cn(
        // 工具条让出的那一条由宿主容器负责（`PrettyView` / `JsonBody` 的外层 `pt-11`），
        // 树只留一点点与首行之间的呼吸，否则两处各让一次，首行会被推得太远。
        'min-h-0 flex-1 overflow-y-auto px-2 pb-2 pt-1 font-mono text-xs',
        wrap ? 'overflow-x-hidden' : 'overflow-x-auto',
      )}
    >
      {/* 不换行时列宽由最长的那行撑开，横向滚动条因此落在本容器上；换行时列宽锁在容器
          内，长字符串才能在自己的行里折行。 */}
      <div className={cn('flex flex-col gap-0.5', wrap ? 'w-full' : 'w-max min-w-full')}>
        {rows.map(({ node, depth }) => {
          const isContainer = node.kind !== 'scalar';
          const isOpen = expanded.has(node.path);
          const label = node.key ?? (node.index === null ? null : String(node.index));
          return (
            <div
              key={node.path}
              className="flex items-start gap-1 rounded-md py-0.5 pr-2 hover:bg-overlay-hover"
              style={{ paddingLeft: `${4 + depth * 14}px` }}
              onContextMenu={(event) => openMenu(event, node)}
            >
              {/* 展开箭头是与行内容并列的独立按钮：按钮不能嵌套按钮。 */}
              {isContainer && node.children.length > 0 ? (
                <button
                  type="button"
                  onClick={() => toggle(node.path)}
                  aria-expanded={isOpen}
                  aria-label={isOpen ? '收起' : '展开'}
                  data-testid="json-tree-toggle"
                  className={cn(
                    'mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded text-faint transition-colors hover:text-foreground',
                    focusRingBare,
                  )}
                >
                  <ChevronRight
                    className={cn('h-3 w-3 transition-transform', isOpen && 'rotate-90')}
                  />
                </button>
              ) : (
                <span aria-hidden className="h-4 w-4 shrink-0" />
              )}
              {/* 行本身不是按钮：它没有点击行为，做成按钮会多出 22 个空的 Tab 停点，
                  按钮里的文本还选不中。右键菜单挂在外层那圈 div 上，行的内容原样留在这里。 */}
              <div data-testid="json-tree-row" className="min-w-0 flex-1 select-text text-left">
                {label !== null && (
                  <>
                    <span className={node.key === null ? 'text-faint' : 'text-foreground'}>
                      {label}
                    </span>
                    <span className="text-muted-foreground">: </span>
                  </>
                )}
                {isContainer ? (
                  <span className="text-muted-foreground">{node.preview}</span>
                ) : (
                  <span
                    className={cn(
                      scalarClass(node.value),
                      wrap ? 'whitespace-pre-wrap break-all' : 'whitespace-pre',
                    )}
                  >
                    {node.scalarText}
                  </span>
                )}
              </div>
            </div>
          );
        })}
      </div>
      <ContextMenu
        open={menu !== null}
        x={menu?.x ?? 0}
        y={menu?.y ?? 0}
        items={items}
        onClose={() => setMenu(null)}
      />
    </div>
  );
}
