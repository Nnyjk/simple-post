import { useCallback, useEffect, useState } from 'react';
import { useAppStore, type NewDraftKind } from '@/stores/app-store';
import type { EditTarget } from '../tree-types';

/**
 * 在创建对话框与树之间架桥：记下每次 `openNewItem()` 调用背后的意图，并在模态框报告新建
 * 行之后，把该行直接丢进就地重命名。
 *
 * 这里没有项目行可供落入，故新建的*项目*跳过重命名那一步 —— `submitNewItem` 已经进入了它，
 * 且名字是在模态框里敲的。
 *
 * 接口根本不走本 hook：`createEndpointAt` 返回它刚创建的 id，调用方直接重命名那一行，无须
 * 经由 `lastCreatedId` 往返一趟。共用这个信号会让「我刚建的那一行」有两个真相来源。
 */
export function useNewItemFlow(setEditing: (t: EditTarget) => void) {
  const openNewItem = useAppStore((s) => s.openNewItem);
  const createEndpointAt = useAppStore((s) => s.createEndpointAt);
  const lastCreatedId = useAppStore((s) => s.lastCreatedId);
  const clearLastCreated = useAppStore((s) => s.clearLastCreated);

  const [lastIntent, setLastIntent] = useState<NewDraftKind | null>(null);

  const openNewItemWithIntent = useCallback(
    (opts: { type: NewDraftKind; parentModuleId?: string; parentCollectionId?: string }) => {
      setLastIntent(opts.type);
      openNewItem(opts);
    },
    [openNewItem],
  );

  /**
   * 在 `parentId` 下创建接口并在工作区打开它。
   *
   * 这里刻意不再有就地重命名。创建接口不再写行：`createEndpointAt` 返回一个 `draft:` 前缀
   * 的 id，该接口在 Save 之前只活在渲染层里（见 `stores/slices/ui-draft-endpoints.ts`）。
   * 没有树行可供落入重命名，而 `setEditing({ kind: 'endpoint', id })` 会把编辑器指向一个
   * 没有行存在的 id —— 树里只会留一个指向不存在行的待重命名目标。
   *
   * 命名这一步跟着行一起搬走了：未命名的草稿改为把工作区标题直接打开在编辑态，那正是
   // 保存前真正读到名字的地方。
   */
  const createEndpoint = useCallback(
    async (parentId: string) => {
      await createEndpointAt(parentId);
    },
    [createEndpointAt],
  );

  useEffect(() => {
    if (!lastCreatedId) return;
    if (!lastIntent) return;
    // 项目在这里没有行，故无可落入的就地重命名 —— 树只列模块 / 集合 / 接口，而
    // `submitNewItem` 已经进入了新项目。进入重命名只会留下一个失效的 `editing` 目标。
    if (lastIntent !== 'project') setEditing({ kind: lastIntent, id: lastCreatedId });
    setLastIntent(null);
    clearLastCreated();
  }, [lastCreatedId, lastIntent, clearLastCreated, setEditing]);

  return { openNewItemWithIntent, createEndpoint };
}
