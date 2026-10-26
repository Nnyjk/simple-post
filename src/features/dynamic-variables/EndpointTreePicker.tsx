/**
 * 数据源接口的选择器 —— 懒加载的层级树 + 跨项目筛选。
 *
 * 动态变量的数据源可以引用任何一个项目里的接口，故这里不按当前项目过滤：树的第一层是
 * 全部项目，展开一个项目才取它的模块，展开一个模块或集合才取它的孩子。每一层都是
 * 「点开才取」，故工作区再大，打开选择器也只读了几行。
 *
 * 筛选框走另一条路：跨全部项目搜接口名、URL 与描述，回来的是命中项与它所在的路径
 * （项目 / 模块 / 集合…）——在大工作区里按名字找一条接口比一层层展开快得多。
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ChevronRight, FolderTree, Loader2, Search } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import type { NodeSearchHit, ResourceNode } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { cn, focusRing } from '@/lib/utils';
import { Input } from '@/components/ui/input';

export interface EndpointTreePickerProps {
  /** 选中的接口 id。空串 = 没选。 */
  value: string;
  /**
   * 选中一个接口。取消选择时后两个参数为空串。
   *
   * 带上接口名与所属项目：表单要把「已选接口」显示成名字，也靠项目 id 去列那个项目的
   * 环境与服务——只有 id 的话界面得自己再查一次树。
   */
  onSelect: (endpointId: string, projectId: string, endpointName: string) => void;
}

/** 树里的一行。项目与节点共用一张表，故用 `kind` 区分。 */
interface TreeRow {
  key: string;
  id: string;
  kind: 'project' | ResourceNode['kind'];
  name: string;
  depth: number;
  /** 这一行所在的项目。接口被选中时要用它，故每行都带着。 */
  projectId: string;
  /** 能否展开。项目与模块、集合都能；接口是叶子。 */
  expandable: boolean;
}

/** 项目行的孩子键。节点的孩子键是 `node:<id>`。 */
function projectKey(projectId: string): string {
  return `project:${projectId}`;
}

/** 节点的孩子键。 */
function nodeKey(nodeId: string): string {
  return `node:${nodeId}`;
}

export function EndpointTreePicker({ value, onSelect }: EndpointTreePickerProps) {
  const projects = useAppStore((s) => s.projects);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [children, setChildren] = useState<Record<string, ResourceNode[]>>({});
  const [loading, setLoading] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState('');
  const [hits, setHits] = useState<NodeSearchHit[]>([]);
  const [searching, setSearching] = useState(false);

  // 筛选：输入停下 250 ms 才发一次请求，免得每敲一个字就跨库搜一遍。
  //
  // 每次输入都起一个新 effect。旧的那次可能已经把请求发出去了，它的回调若照写状态，
  // 就会把上一轮的结果盖到这一轮上——故用 `cancelled` 标出「我这一轮已经过期」。
  const searchTimer = useRef<number | null>(null);
  useEffect(() => {
    const text = query.trim();
    let cancelled = false;
    if (searchTimer.current !== null) window.clearTimeout(searchTimer.current);
    if (text === '') {
      setHits([]);
      setSearching(false);
      return;
    }
    setSearching(true);
    searchTimer.current = window.setTimeout(() => {
      void invokeCmd<NodeSearchHit[]>(COMMANDS.nodesSearchPaths, { query: text, limit: 50 })
        .then((rows) => {
          if (!cancelled) setHits(rows);
        })
        .catch((e: unknown) => {
          console.error('[dynamic-variables] node search failed', e);
          if (!cancelled) setHits([]);
        })
        .finally(() => {
          if (!cancelled) setSearching(false);
        });
    }, 250);
    return () => {
      cancelled = true;
      if (searchTimer.current !== null) window.clearTimeout(searchTimer.current);
    };
  }, [query]);

  /** 展开一行：第一次展开才去取孩子。 */
  const toggle = useCallback((key: string, load: () => Promise<void>) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
        return next;
      }
      next.add(key);
      return next;
    });
    void load();
  }, []);

  const loadProjectChildren = useCallback(
    async (projectIdToLoad: string, key: string) => {
      if (children[key] || loading.has(key)) return;
      setLoading((prev) => new Set(prev).add(key));
      try {
        const rows = await invokeCmd<ResourceNode[]>(COMMANDS.nodesChildren, {
          projectId: projectIdToLoad,
        });
        setChildren((prev) => ({ ...prev, [key]: rows }));
      } catch (e) {
        console.error('[dynamic-variables] load project children failed', e);
        setChildren((prev) => ({ ...prev, [key]: [] }));
      } finally {
        setLoading((prev) => {
          const next = new Set(prev);
          next.delete(key);
          return next;
        });
      }
    },
    [children, loading],
  );

  const loadNodeChildren = useCallback(
    async (nodeId: string, key: string) => {
      if (children[key] || loading.has(key)) return;
      setLoading((prev) => new Set(prev).add(key));
      try {
        const rows = await invokeCmd<ResourceNode[]>(COMMANDS.nodesChildren, {
          parentId: nodeId,
        });
        setChildren((prev) => ({ ...prev, [key]: rows }));
      } catch (e) {
        console.error('[dynamic-variables] load node children failed', e);
        setChildren((prev) => ({ ...prev, [key]: [] }));
      } finally {
        setLoading((prev) => {
          const next = new Set(prev);
          next.delete(key);
          return next;
        });
      }
    },
    [children, loading],
  );

  /** 可见的行：根是全部项目，展开的分支按深度依次铺开。 */
  const rows = useMemo(() => {
    const out: TreeRow[] = [];
    const pushNode = (node: ResourceNode, depth: number) => {
      const key = nodeKey(node.id);
      out.push({
        key,
        id: node.id,
        kind: node.kind,
        name: node.name,
        depth,
        projectId: node.rootId,
        expandable: node.kind !== 'endpoint',
      });
      if (!expanded.has(key)) return;
      for (const child of children[key] ?? []) pushNode(child, depth + 1);
    };
    for (const project of projects) {
      const key = projectKey(project.id);
      out.push({
        key,
        id: project.id,
        kind: 'project',
        name: project.name,
        depth: 0,
        projectId: project.id,
        expandable: true,
      });
      if (!expanded.has(key)) continue;
      for (const child of children[key] ?? []) pushNode(child, 1);
    }
    return out;
  }, [projects, expanded, children]);

  const filtering = query.trim() !== '';

  return (
    <div className="flex flex-col gap-2">
      <div className="relative">
        <Search className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
        <Input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="按名字或 URL 找接口"
          className="h-8 pl-7 text-xs"
        />
        {searching && (
          <Loader2 className="absolute right-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 animate-spin text-muted-foreground" />
        )}
      </div>

      <div className="max-h-56 overflow-y-auto rounded-md border border-border">
        {filtering ? (
          hits.length === 0 ? (
            <p className="px-3 py-4 text-center text-xs text-muted-foreground">
              {searching ? '搜索中…' : '没有匹配的接口。'}
            </p>
          ) : (
            <ul>
              {hits.map((hit) => (
                <li key={hit.node.id}>
                  <button
                    type="button"
                    onClick={() => onSelect(hit.node.id, hit.node.rootId, hit.node.name)}
                    className={cn(
                      'flex w-full flex-col items-start gap-0.5 px-2 py-1.5 text-left transition-colors hover:bg-accent',
                      focusRing,
                      value === hit.node.id && 'bg-accent',
                    )}
                  >
                    <span className="w-full truncate text-xs font-medium">{hit.node.name}</span>
                    <span className="w-full truncate text-[11px] text-muted-foreground">
                      {hit.path.join(' / ')}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )
        ) : rows.length === 0 ? (
          <p className="px-3 py-4 text-center text-xs text-muted-foreground">
            还没有项目。先建一个项目，再从它的树里挑一个接口。
          </p>
        ) : (
          <ul>
            {rows.map((row) => {
              const isExpanded = expanded.has(row.key);
              const busy = loading.has(row.key);
              const kids = children[row.key];
              return (
                <li key={row.key}>
                  <div
                    className="flex items-center gap-1 px-1 py-0.5 text-xs"
                    style={{ paddingLeft: `${row.depth * 12 + 4}px` }}
                  >
                    {row.expandable ? (
                      <button
                        type="button"
                        onClick={() =>
                          toggle(row.key, () =>
                            row.kind === 'project'
                              ? loadProjectChildren(row.id, row.key)
                              : loadNodeChildren(row.id, row.key),
                          )
                        }
                        className={cn(
                          'flex h-5 w-5 shrink-0 items-center justify-center rounded-sm text-faint transition-colors hover:bg-accent hover:text-foreground',
                          focusRing,
                        )}
                        aria-label={isExpanded ? '收起' : '展开'}
                      >
                        {busy ? (
                          <Loader2 className="h-3 w-3 animate-spin" />
                        ) : (
                          <ChevronRight
                            className={cn(
                              'h-3 w-3 transition-transform',
                              isExpanded && 'rotate-90',
                            )}
                          />
                        )}
                      </button>
                    ) : (
                      <span className="h-5 w-5 shrink-0" />
                    )}
                    {row.kind === 'endpoint' ? (
                      <button
                        type="button"
                        onClick={() => onSelect(row.id, row.projectId, row.name)}
                        className={cn(
                          'min-w-0 flex-1 truncate rounded-sm px-1 py-0.5 text-left transition-colors hover:bg-accent',
                          focusRing,
                          value === row.id && 'bg-accent font-medium',
                        )}
                      >
                        {row.name}
                      </button>
                    ) : (
                      <span className="min-w-0 flex-1 truncate px-1 py-0.5 text-muted-foreground">
                        {row.name}
                      </span>
                    )}
                    {isExpanded && kids && kids.length === 0 && (
                      <span className="pr-1 text-[11px] text-faint">（空）</span>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </div>

      <p className="flex items-center gap-1 text-[11px] text-muted-foreground">
        <FolderTree className="h-3 w-3" />
        {value === ''
          ? '还没选接口。展开一个项目，逐层找到要取数的那条接口。'
          : '已选好接口。在上面的框里输入名字可以跨项目找。'}
      </p>
    </div>
  );
}
