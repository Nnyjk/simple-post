import type { EndpointDetail, ResourceNode } from '@/types/domain';
import { indexByParent } from '@/lib/tree-index';
import type { CollectionBranch, DragPayload, TreeBranch } from './tree-types';

/**
 * 应用内拖拽负载的 mime 类型。把被拖的 kind + ids 以 JSON 塞进 dataTransfer，
 * 一次 drop 就能解析成 `moveNodes`，无需额外上下文。用带厂商前缀的类型，
 * 别的应用 / 操作系统就不会把它当文本捡走。
 */
export const DND_MIME = 'application/x-simple-post-tree';

export function encodeDrag(payload: DragPayload): string {
  return JSON.stringify(payload);
}

export function decodeDrag(raw: string): DragPayload | null {
  try {
    const parsed = JSON.parse(raw) as DragPayload;
    if (!parsed || typeof parsed !== 'object') return null;
    if (
      (parsed.kind === 'module' || parsed.kind === 'collection' || parsed.kind === 'endpoint') &&
      Array.isArray(parsed.ids) &&
      parsed.ids.every((id) => typeof id === 'string')
    ) {
      return parsed;
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * 把打平的节点列表变成行渲染所用的嵌套形状，一趟建完。
 *
 * 每个节点都指明自己的父节点，整棵树就是单个 `parentId → children` 索引。
 *
 * `projectId` 收窄到一个项目的树；其他项目的节点直接丢弃，而不是渲染到不属于自己的
 * 模块底下。
 */
export function buildTree(
  nodes: readonly ResourceNode[],
  endpointDetails: Record<string, EndpointDetail>,
  projectId: string,
): TreeBranch[] {
  // 分桶本身与 store 共用同一个索引，只是收窄到本项目而非整个工作区。
  const byParent = indexByParent(nodes, projectId);

  const childrenOf = (
    parentId: string,
  ): { endpoints: ResourceNode[]; collections: ResourceNode[] } => {
    const kids = byParent.get(parentId) ?? [];
    return {
      // 一次过滤覆盖两种容器类型：模块下可放集合*和*接口（即无文件夹的情形），集合下可放
      // 接口和更多集合。别的类型都进不来，故无需按类型分别查询。
      endpoints: kids.filter((k) => k.kind === 'endpoint'),
      collections: kids.filter((k) => k.kind === 'collection'),
    };
  };

  /** store 的 detail map 中覆盖 `endpoints` 的那一部分。 */
  const detailsFor = (endpoints: readonly ResourceNode[]): Record<string, EndpointDetail> => {
    const out: Record<string, EndpointDetail> = {};
    for (const e of endpoints) {
      const d = endpointDetails[e.id];
      if (d) out[e.id] = d;
    }
    return out;
  };

  const buildCollectionBranch = (c: ResourceNode): CollectionBranch => {
    const { endpoints, collections } = childrenOf(c.id);
    // `childrenOf` 只返回本分支自己的接口，不含整棵子树；集合行渲染的就是这些接口。
    return {
      collection: c,
      endpoints,
      endpointDetails: detailsFor(endpoints),
      children: collections.map(buildCollectionBranch),
    };
  };

  // 模块的父节点是项目，故顶层模块就是本项目里没有 `parentId` 的那些节点。
  return (byParent.get(null) ?? [])
    .filter((n) => n.kind === 'module')
    .map((m) => {
      const { endpoints, collections } = childrenOf(m.id);
      return {
        module: m,
        endpoints,
        endpointDetails: detailsFor(endpoints),
        collections: collections.map(buildCollectionBranch),
      };
    });
}

/**
 * 该接口是否命中搜索：名称、url 或任一 tag。url 与 tag 都在 detail 行里，
 * 只在节点上匹配名称会让「按路径找接口」悄悄失效。
 *
 * 模块级与集合级过滤共用它，两个层级不会漂移成匹配不同的东西。
 */
function endpointMatches(
  e: ResourceNode,
  q: string,
  details: Record<string, EndpointDetail>,
): boolean {
  if (e.name.toLowerCase().includes(q)) return true;
  const d = details[e.id];
  if (!d) return false;
  return d.url.toLowerCase().includes(q) || d.tags.some((t) => t.toLowerCase().includes(q));
}

/** 只留命中的接口，以及支撑它们的 detail 行。 */
function filterEndpoints(
  endpoints: ResourceNode[],
  q: string,
  details: Record<string, EndpointDetail>,
): { endpoints: ResourceNode[]; endpointDetails: Record<string, EndpointDetail> } {
  const matched = endpoints.filter((e) => endpointMatches(e, q, details));
  const filteredDetails: Record<string, EndpointDetail> = {};
  for (const e of matched) {
    const d = details[e.id];
    if (d) filteredDetails[e.id] = d;
  }
  return { endpoints: matched, endpointDetails: filteredDetails };
}

export function filterTree(
  tree: TreeBranch[],
  q: string,
  details: Record<string, EndpointDetail>,
): TreeBranch[] {
  return (
    tree
      .map((m) => {
        const collections = filterCollectionBranch(m.collections, q, details);
        // 模块可以直接持有接口，故只过滤它的集合会让这些接口再也搜不到。
        const { endpoints, endpointDetails } = filterEndpoints(m.endpoints, q, details);
        return { ...m, collections, endpoints, endpointDetails };
      })
      // 模块只要有命中的集合*或*自己名下命中的接口就留下。
      .filter((m) => m.collections.length > 0 || m.endpoints.length > 0)
  );
}

function filterCollectionBranch(
  branches: CollectionBranch[],
  q: string,
  details: Record<string, EndpointDetail>,
): CollectionBranch[] {
  return branches
    .map((b) => {
      const { endpoints, endpointDetails } = filterEndpoints(b.endpoints, q, details);
      const filteredChildren = filterCollectionBranch(b.children, q, details);
      // 集合有命中的接口、命中的子集合、或自身名称命中，三者之一即保留。
      const nameMatches = b.collection.name.toLowerCase().includes(q);
      if (endpoints.length === 0 && filteredChildren.length === 0 && !nameMatches) {
        return null;
      }
      return {
        ...b,
        endpoints,
        endpointDetails,
        children: filteredChildren,
      };
    })
    .filter((b): b is CollectionBranch => b !== null);
}
