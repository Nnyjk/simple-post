import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react';
import { Braces, ChevronRight, Copy, KeyRound, Route } from 'lucide-react';
import { cn, focusRingBare } from '@/lib/utils';
import {
  defaultExpandedPaths,
  flattenTree,
  pairText,
  subtreeJson,
  valueText,
  type JsonTreeNode,
  type JsonValue,
} from '@/lib/json-tree';
import { ContextMenu, type ContextMenuItem } from './context-menu';

export interface JsonTreeViewProps {
  /**
   * 已解析的树；调用方 `useMemo` 后传入。
   *
   * 解析放在调用方是有意的：调用方本来就要在同一份文本上判一次「是不是合法 JSON」，
   * 树这边再解析一次就是同一份文本解析两遍。
   */
  root: JsonTreeNode | null;
  /** 长字符串值是否在内容宽度内折行；默认 false，靠横向滚动看完整内容。 */
  wrap?: boolean;
  /**
   * 是否处于可见档位；默认 true。
   *
   * 切到别的档位时菜单要一并关掉：`ContextMenu` portal 在 `document.body` 上，宿主藏起
   * 树是藏不住菜单的，留着它还能从看不见的树上取值、还会截走 Escape。
   */
  active?: boolean;
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
export function JsonTreeView({ root, wrap = false, active = true }: JsonTreeViewProps) {
  const [expanded, setExpanded] = useState<Set<string>>(
    () => new Set(root ? defaultExpandedPaths(root) : []),
  );
  // `menu` 声明在下面那个重置 effect 之前：那个 effect 要一并关掉菜单。
  const [menu, setMenu] = useState<{ node: JsonTreeNode; x: number; y: number } | null>(null);
  const [copyFailed, setCopyFailed] = useState(false);
  const failTimer = useRef<number | null>(null);
  /** 复制尝试的序号：只有最后一次尝试有权改提示，先发后至的旧结论直接丢掉。 */
  const attempt = useRef(0);

  // 文本换了一棵树：展开状态交回默认值，旧路径的痕迹不会留到新树上；菜单里那几项指向的
  // 是上一棵树的节点，留着会把已经不在屏幕上的内容复制进剪贴板。
  useEffect(() => {
    setExpanded(new Set(root ? defaultExpandedPaths(root) : []));
    setMenu(null);
  }, [root]);

  // 档位切走了（树被 `display: none` 藏起来）：菜单挂在 body 上，不跟着藏，必须自己关。
  useEffect(() => {
    if (active) return;
    setMenu(null);
  }, [active]);

  // 卸载时清掉未到期的计时器，否则它会在已卸载的组件上 setState。
  useEffect(
    () => () => {
      if (failTimer.current !== null) window.clearTimeout(failTimer.current);
    },
    [],
  );

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

  /**
   * 复制一项。剪贴板是异步的，且可能整条不可用（非安全上下文、权限被拒）—— 失败时如实
   * 报失败，不能先点亮「已复制」再让内容留在旧剪贴板里。
   */
  const copy = (value: string) => {
    const id = ++attempt.current;
    void (async () => {
      try {
        if (!navigator.clipboard) throw new Error('剪贴板不可用');
        await navigator.clipboard.writeText(value);
        // 这一次已经被后来的尝试顶掉：提示归那一次管，这里不许再改。
        if (id !== attempt.current) return;
        // 重试成功了，上一轮那条失败提示连同它的计时器一起作废 —— 留着它会在成功后
        // 继续挂 2.4s，用户刚复制成功却仍看到「复制失败」。
        if (failTimer.current !== null) {
          window.clearTimeout(failTimer.current);
          failTimer.current = null;
        }
        setCopyFailed(false);
      } catch {
        if (id !== attempt.current) return;
        // 菜单选中即关闭，提示必须落在菜单之外 —— 故挂在树的滚动区上方。
        setCopyFailed(true);
        if (failTimer.current !== null) window.clearTimeout(failTimer.current);
        failTimer.current = window.setTimeout(() => setCopyFailed(false), 2400);
      }
    })();
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
          onClick: () => copy(subtreeJson(menu.node)),
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

  /**
   * 键盘打开同一份菜单。标量行与空容器没有展开箭头，若不在这里接住菜单键，键盘用户
   * 就永远够不到「复制 JSON 路径」。
   */
  const openMenuByKeyboard = (event: KeyboardEvent<HTMLDivElement>, node: JsonTreeNode) => {
    if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return;
    event.preventDefault();
    const rect = event.currentTarget.getBoundingClientRect();
    setMenu({ node, x: rect.left + 24, y: rect.bottom });
  };

  if (!root) return null;

  const textWrap = wrap ? 'whitespace-pre-wrap break-all' : 'whitespace-pre';

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* 这一条 `role="status"` 常驻，只有文案是条件渲染：读屏只播报「已有的 live
          region 里新增的内容」，容器与消息同一次渲染才出现的，常常一句都不念。 */}
      <div role="status" className="shrink-0 px-2 pt-1 text-destructive text-nano">
        {copyFailed ? '复制失败：无法访问剪贴板' : null}
      </div>
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
                // 行是靠右键菜单取值的，键盘也要有等价入口：聚焦整行后按菜单键 / Shift+F10。
                tabIndex={0}
                aria-haspopup="menu"
                className={cn(
                  'flex items-start gap-1 rounded-md py-0.5 pr-2 hover:bg-overlay-hover',
                  focusRingBare,
                )}
                style={{ paddingLeft: `${4 + depth * 14}px` }}
                onContextMenu={(event) => openMenu(event, node)}
                onKeyDown={(event) => openMenuByKeyboard(event, node)}
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
                      <span
                        className={cn(
                          node.key === null ? 'text-faint' : 'text-foreground',
                          textWrap,
                        )}
                      >
                        {label}
                      </span>
                      <span className="text-muted-foreground">: </span>
                    </>
                  )}
                  {isContainer ? (
                    <span className="text-muted-foreground">{node.preview}</span>
                  ) : (
                    <span className={cn(scalarClass(node.value), textWrap)}>{node.scalarText}</span>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </div>
      <ContextMenu
        open={menu !== null && active}
        x={menu?.x ?? 0}
        y={menu?.y ?? 0}
        items={items}
        onClose={() => setMenu(null)}
      />
    </div>
  );
}
