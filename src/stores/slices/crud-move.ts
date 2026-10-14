/**
 * CRUD 子工厂 —— 拖放移动。
 *
 * `createCrudSlice` 组合的五个子工厂之一（切片级的契约——乐观 `set()` →
 * `invokeCmd` → 快照回滚——见 `../crud-slice.ts`）。
 *
 * 移动是一个动作而不是三个，而且无法表达成一次普通的字段写入：本地 `set()`
 * 会重算目标同级节点的 `sortOrder`，并把被移动子树里每个后代的 `rootId`
 * 一起带走。`ResourceNodeType::may_under` 这条契约——module 只能顶层、endpoint
 * 只能挂在 collection 或 module 下、collection 只能挂在 module 或 collection
 * 下——由 Rust 里的触发器强制；下面的守卫是它的乐观镜像，所以被拒的拖放
 * 绝不会渲染出后端本来就不会接受的状态。
 */
import type { StateCreator } from 'zustand';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { indexByParent } from '@/lib/tree-index';
import type { AppState } from '../app-store';
import { collectSubtree, renumber } from './crud-helpers';

/** CRUD 切片拥有的移动动作。 */
export interface MoveCrudActions {
  /**
   * 把一个或多个节点移到 `parentId` 下的同级槽位 `index`。
   *
   * `parentId: null` 表示"项目顶层"，且只对 module 合法。`nodeIds` 按节点应当
   * 落地的先后排列，所以多选拖放会保持用户挑选的顺序。
   */
  moveNodes: (nodeIds: string[], parentId: string | null, index: number) => Promise<void>;
}

export const createMoveCrud: StateCreator<AppState, [], [], MoveCrudActions> = (set, get) => ({
  moveNodes: async (nodeIds, parentId, index) => {
    const prevNodes = get().nodes;
    set((s) => {
      if (nodeIds.length === 0) return {};
      const idSet = new Set(nodeIds);
      const childrenIndex = indexByParent(s.nodes);
      const sources = s.nodes.filter((n) => idSet.has(n.id));
      if (sources.length === 0) return {};

      // 目标父节点，以及这些节点将落入的项目。`rootId` 就在每个节点身上，
      // 所以项目边界检查是一次字符串比较，不用像以前那样一路向上走到 module。
      let targetRootId: string;
      if (parentId === null) {
        // "项目顶层"——只对 module 合法。
        if (sources.some((n) => n.kind !== 'module')) return {};
        targetRootId = sources[0].rootId;
      } else {
        const targetParent = s.nodes.find((n) => n.id === parentId);
        if (!targetParent) return {};
        targetRootId = targetParent.rootId;
      }

      // 防环：节点不能被移到它自己或它的任何后代之下。UI 会禁用这类落点，
      // 但陈旧的拖放载荷仍可能报出这样一个 id。
      const movedSubtrees = new Set<string>();
      for (const src of sources) {
        for (const nid of collectSubtree(src.id, childrenIndex)) movedSubtrees.add(nid);
      }
      if (parentId !== null && movedSubtrees.has(parentId)) return {};

      // 每个源节点必须已经属于目标所在的项目。服务端否则会拒绝这次移动；
      // 在本地就拒绝，免得乐观状态显示出将会 400 的东西。
      for (const src of sources) {
        if (src.rootId !== targetRootId) return {};
      }

      // 剔掉被移动者的目标同级节点——它们会在下方重新插到 `index` 处。
      //
      // 落到顶层时，同级就是目标项目的 module，也就是那些没有父节点的节点；
      // 只按 `parentId === null` 界定会把所有项目的顶层一次性扫进来。
      const siblings =
        parentId === null
          ? s.nodes
              .filter((n) => n.parentId === null && n.rootId === targetRootId && !idSet.has(n.id))
              .sort((a, b) => a.sortOrder - b.sortOrder)
          : (childrenIndex.get(parentId) ?? []).filter((n) => !idSet.has(n.id));

      // 被移动的行保持调用方给的顺序，所以多选拖放会按用户挑选的次序落地。
      const movedRows = sources
        .slice()
        .sort((a, b) => nodeIds.indexOf(a.id) - nodeIds.indexOf(b.id))
        .map((n) => ({ ...n, parentId, rootId: targetRootId }));

      const clampedIndex = Math.max(0, Math.min(index, siblings.length));
      const renumbered = renumber([
        ...siblings.slice(0, clampedIndex),
        ...movedRows,
        ...siblings.slice(clampedIndex),
      ]);
      const renumberedById = new Map(renumbered.map((n) => [n.id, n] as const));

      // 每个节点要么保住原槽位（在目标之外），要么是被重新编号的行，
      // 要么是某个被移动行的后代——后者的 `rootId` 必须跟着走。只重排目标
      // 那一层会漏掉最后这一类，导致跨项目移动子树的乐观状态与数据库不一致。
      const next = s.nodes.map((n) => {
        const renamed = renumberedById.get(n.id);
        if (renamed) return renamed;
        if (movedSubtrees.has(n.id) && n.rootId !== targetRootId) {
          return { ...n, rootId: targetRootId };
        }
        return n;
      });

      return { nodes: next };
    });
    try {
      await invokeCmd<void>(COMMANDS.nodesMove, { nodeIds, parentId, index });
    } catch (e) {
      console.error('[store] nodes_move failed', e);
      set({ nodes: prevNodes });
    }
  },
});
