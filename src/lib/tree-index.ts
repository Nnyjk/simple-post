import type { ResourceNode } from '@/types/domain';

/**
 * 资源树赖以构建的 `parentId → children` 索引，全应用**只在这里**建一次。
 *
 * 每个节点自己写明父节点，故模块 / 集合 / endpoint 整棵树就是一张 `parentId → children` 表，
 * 而不是三张各自要 join 的表。store 靠它走子树、找一次移动的目的地同级兄弟；渲染器的
 * `buildTree` 靠同一份再收窄到单个项目。组件里就地过滤扁平列表拿 children 等于重建一份。
 *
 * 调用方依赖的两个不变式：
 *
 *   * **纯。** 返回全新的 map 与全新的数组，节点对象按引用持有，不重排也不写回调用方的数据
 *     ——故在 `set()` 回调里构建是安全的。
 *   * **不是 selector。** 返回的 map 每次调用都是新对象，而 Zustand v5 用 `Object.is` 比较：
 *     把这种东西交给 selector 会无限重渲染。请放进 `useMemo` 或事件处理器。
 *
 * `sortOrder` 的作用域是**父节点**，故每个桶各自排序；同一个数字在两个不同父节点下可以代表
 * 不同位置，两者都要保留。
 */
export function indexByParent(
  nodes: readonly ResourceNode[],
  /**
   * 收窄到单个项目的树。省略即整份工作区——store 就是这种用法：级联删除或一次移动必须一次
   * 看到所有项目的节点，而不只屏幕上的那个。
   */
  rootId?: string,
): Map<string | null, ResourceNode[]> {
  const byParent = new Map<string | null, ResourceNode[]>();
  for (const n of nodes) {
    if (rootId !== undefined && n.rootId !== rootId) continue;
    const bucket = byParent.get(n.parentId);
    if (bucket) bucket.push(n);
    else byParent.set(n.parentId, [n]);
  }
  for (const bucket of byParent.values()) {
    bucket.sort((a, b) => a.sortOrder - b.sortOrder);
  }
  return byParent;
}

/**
 * endpoint 所在路径上的容器名 —— 从模块往下逐层列出，最外层在前。
 *
 * 标签条用它给同名接口做区分：两个接口重名时，光看名字分不出谁是谁，路径能。
 *
 * 只列**容器**（模块与集合），不含接口自己：调用方已经知道接口名，它要回答的是
 * 「这个接口在树的哪里」。直接挂在模块下的接口得到一段路径（模块名），挂在集合下的得到
 * 两段（模块名、集合名），集合可以嵌套任意层，故路径长度不固定。
 *
 * 收全量节点列表并自己做索引，调用方可以直接传 `store.nodes`；它**没有**做 memo：
 * 返回值是新数组，请放进 `useMemo`，不要当 Zustand selector 用。
 *
 * 找不到祖先时返回空数组：接口还没有容器（草稿的父节点还没建出来）或树还没加载完时，
 * 那是「路径暂缺」而不是「路径为空」，调用方据此不写提示。
 */
export function containerPathOf(
  nodes: readonly ResourceNode[],
  parentId: string | null | undefined,
): string[] {
  if (!parentId) return [];
  const byId = new Map<string, ResourceNode>();
  for (const n of nodes) byId.set(n.id, n);

  const path: string[] = [];
  const seen = new Set<string>();
  let cur = byId.get(parentId);
  // `seen` 让遍历在成环处停下：每个节点 id 最多被访问一次，而节点总数有限，故遍历必然终止。
  // 集合可以嵌套任意深（schema 不限嵌套层数），故这里不另设层数上限——设了会在很深的
  // 嵌套下把最外层的模块名丢掉，而那正是区分同名接口最有用的一段。
  while (cur && !seen.has(cur.id)) {
    seen.add(cur.id);
    if (cur.kind === 'collection' || cur.kind === 'module') path.unshift(cur.name);
    // 模块是路径的顶端：它上面只有项目，而项目不是容器。
    if (cur.kind === 'module') break;
    cur = cur.parentId ? byId.get(cur.parentId) : undefined;
  }
  return path;
}

/**
 * endpoint 最终所处模块的 id，不在任何模块下则为 `null`。
 *
 * 项目 id 以 `rootId` 冗余写在每个节点上，「属于哪个项目」一次查得到；但模块没有对应列，
 * 故这里走 `parentId`。DB 侧这个不对称是有意的（模块随移动变，项目不变），此处代价也低：
 * 遍历长度受集合嵌套深度约束，而 `nodes` 已在内存。
 *
 * 之所以需要它：baseUrl 的**值**是按模块的——endpoint 通过 `env.moduleBaseUrls[moduleId]`
 * 解析自己的地址（见 `lib/url.ts` 的 `resolveEndpointBaseUrl`），解析层得先知道它属于哪个模块
 * 才能读那个格子。那个格子是 (环境 × 模块) 独有的，故没有模块就没有格子可读。
 *
 * 收全量节点列表并自己做索引，调用方可以直接传 `store.nodes`；它**没有**做 memo：请放进
 * `useMemo`，不要当 Zustand selector 用（返回的字符串在 `Object.is` 下是稳定的，但遍历本身
 * 不该每次渲染都重跑）。
 *
 * **草稿不在 `nodes` 里。** 未保存的 endpoint 在 Save 前只存在于 `draftEndpoints`，
 * `byId.get(draftId)` 查不到，遍历便返回 `null`——下游读起来就是「这个 endpoint 没有模块」，
 * 也就是根本没有 baseUrl 格子。它的各级父节点都是真实行，缺的只是起点：把草稿自身的节点作为
 * `self` 传进来。
 */
export function moduleIdOfEndpoint(
  nodes: readonly ResourceNode[],
  endpointId: string | null | undefined,
  /**
   * `nodes` 里没有它时传入的 endpoint 自身行——即未保存的草稿，由
   * `useActiveEndpointResource` 从 `DraftEndpoint` 合成。`nodes` 里已有 `endpointId` 对应行时
   * 忽略本参数，故始终传 `resource.node` 的调用方对两种存储形态都正确。
   */
  self?: ResourceNode | null,
): string | null {
  if (!endpointId) return null;
  const byId = new Map<string, ResourceNode>();
  for (const n of nodes) byId.set(n.id, n);

  // `kind` 是节点自身的类型；endpoint 可以直接挂在 module 下（无集合），故这两种形态都由
  // 这个循环终止
  let cur = byId.get(endpointId) ?? (self?.id === endpointId ? self : undefined);
  let guard = 0;
  while (cur && guard++ < 64) {
    if (cur.kind === 'module') return cur.id;
    if (!cur.parentId) return null;
    cur = byId.get(cur.parentId);
  }
  // 成环不可能——schema 的父子矩阵与移动守卫都禁止它——但这个有界遍历静默返回 `null`，
  // 好过一次无界遍历把发送路径挂死
  return null;
}
