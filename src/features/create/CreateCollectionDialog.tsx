/**
 * CreateCollectionDialog —— 挂在 `ProjectTree` 内。
 *
 * 多数入口是某一行的 `+`，而那一行本身就是答案：集合要挂在那个模块或那个集合下。
 * 故父级下拉仅在「未预选集合」时出现（树的 新建模块 式入口、命令面板）；预选时改为
 * 陈述这行会落在哪，让嵌套层级可读。
 *
 * 提示里那个模块是上下文而非第二个输入：`submitNewItem` 先解析 `parentCollectionId`。
 */

import { useEffect, useMemo, useRef } from 'react';
import { useAppStore } from '@/stores/app-store';
import { Select } from '@/components/ui/select';
import type { ResourceNode } from '@/types/domain';
import { CreateDialog, DescriptionField, NameField } from './CreateDialog';

export function CreateCollectionDialog() {
  const draft = useAppStore((s) => s.newItem);
  const nodes = useAppStore((s) => s.nodes);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const closeNewItem = useAppStore((s) => s.closeNewItem);
  const setNewItemName = useAppStore((s) => s.setNewItemName);
  const setNewItemDescription = useAppStore((s) => s.setNewItemDescription);
  const setNewItemParentModule = useAppStore((s) => s.setNewItemParentModule);
  const submitNewItem = useAppStore((s) => s.submitNewItem);

  const nameInputRef = useRef<HTMLInputElement>(null);
  const open = draft.open && draft.type === 'collection';

  const projectModules = useMemo(
    () =>
      nodes
        .filter((n) => n.kind === 'module' && n.rootId === activeProjectId)
        .sort((a, b) => a.sortOrder - b.sortOrder),
    [nodes, activeProjectId],
  );
  const moduleOptions = useMemo(
    () => projectModules.map((m) => ({ value: m.id, label: m.name })),
    [projectModules],
  );

  // 点击处预选的集合，以及它所属的模块 —— 提示同时点出两者，嵌套层级才读得出来。
  const preselected = useMemo(() => {
    if (!draft.parentCollectionId) return null;
    const col = nodes.find((n) => n.id === draft.parentCollectionId);
    if (!col || col.kind !== 'collection') return null;
    let cur: ResourceNode | undefined = col;
    const seen = new Set<string>([col.id]);
    while (cur && cur.kind !== 'module' && !seen.has(cur.id)) {
      seen.add(cur.id);
      cur = cur.parentId ? nodes.find((n) => n.id === cur!.parentId) : undefined;
    }
    return { collection: col, module: cur && cur.kind === 'module' ? cur : null };
  }, [nodes, draft.parentCollectionId]);

  // 默认选项目第一个模块，不动下拉也能直接点创建。`!open` 守卫确保绝不写到
  // 屏幕上看不见的草稿。
  useEffect(() => {
    if (!open || preselected) return;
    if (draft.parentModuleId) return;
    if (projectModules.length === 0) return;
    setNewItemParentModule(projectModules[0].id);
  }, [open, preselected, draft.parentModuleId, projectModules, setNewItemParentModule]);

  const hasNoModules = projectModules.length === 0 && !preselected;
  const canSubmit = draft.name.trim().length > 0 && (Boolean(preselected) || !hasNoModules);

  const submit = () => {
    if (!canSubmit) return;
    void submitNewItem();
    closeNewItem();
  };

  return (
    <CreateDialog
      open={open}
      onClose={closeNewItem}
      title="新建集合"
      description="集合用于归类接口"
      canSubmit={canSubmit}
      onSubmit={submit}
      nameInputRef={nameInputRef}
    >
      <NameField value={draft.name} onChange={setNewItemName} inputRef={nameInputRef} />
      <DescriptionField value={draft.description} onChange={setNewItemDescription} />
      {preselected ? (
        <p className="text-xs text-muted-foreground">
          将创建在
          <span className="text-foreground">
            {preselected.module ? `${preselected.module.name} / ` : ''}
            {preselected.collection.name}
          </span>
          下
        </p>
      ) : (
        <>
          <label className="block space-y-1">
            <span className="text-xs font-medium text-muted-foreground">
              所属模块
              <span className="ml-0.5 text-destructive">*</span>
            </span>
            <Select
              value={draft.parentModuleId}
              onChange={(e) => setNewItemParentModule(e.target.value)}
              options={moduleOptions}
              disabled={hasNoModules}
            />
          </label>
          {hasNoModules && (
            <p className="text-xs text-muted-foreground">当前项目还没有模块，请先新建一个模块。</p>
          )}
        </>
      )}
    </CreateDialog>
  );
}
