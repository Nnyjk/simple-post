import { useCallback } from 'react';
import { useAppStore, type NewDraftKind } from '@/stores/app-store';

/**
 * 树的「新建」入口：把右键菜单 / 行尾「+」的意图交给创建对话框，以及那条完全不用
 * 对话框的接口路径。
 *
 * 这里没有就地重命名要做 —— 名字属于打开着的工作区（接口标题与项目卡片各自可编辑），
 * 树行只是一行只读的名字。
 */
export function useNewItemFlow() {
  const openNewItem = useAppStore((s) => s.openNewItem);
  const createEndpointAt = useAppStore((s) => s.createEndpointAt);

  const openNewItemWithIntent = useCallback(
    (opts: { type: NewDraftKind; parentModuleId?: string; parentCollectionId?: string }) => {
      openNewItem(opts);
    },
    [openNewItem],
  );

  /**
   * 在 `parentId` 下创建一个接口草稿并在工作区打开它。
   *
   * 草稿在 Save 之前只活在渲染层里，故没有树行可操作：命名跟着工作区标题走。
   */
  const createEndpoint = useCallback(
    async (parentId: string) => {
      await createEndpointAt(parentId);
    },
    [createEndpointAt],
  );

  return { openNewItemWithIntent, createEndpoint };
}
