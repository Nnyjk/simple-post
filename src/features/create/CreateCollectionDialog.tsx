/**
 * CreateCollectionDialog — mounted inside `ProjectTree`.
 *
 * THE PARENT IS USUALLY ALREADY DECIDED
 * ------------------------------------
 * Two thirds of the entry points are a row's `+` button, and the row you
 * clicked *is* the answer: the collection belongs under that module or
 * under that collection. Re-asking it in the dialog meant the draft could
 * disagree with the click — which is how 「新建子集合」 used to create a
 * sibling under the module instead of a child under the collection.
 *
 * So the parent select only appears when the dialog was opened with no
 * collection preselected (the tree's own 「新建模块」-style entry points
 * and the command palette). When a collection *was* preselected the form
 * states where the row will land instead of offering to change it.
 *
 * That also means the module shown in that hint is context, not a second
 * input: `submitNewItem` resolves `parentCollectionId` first.
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

  // The collection the click site preselected, plus the module that owns
  // it — the hint names both so the nesting is legible ("… / 订单 / 退款").
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

  // Default to the project's first module so the create button is
  // reachable without touching the select. Guarded on `!open` so it can
  // never write to a draft that isn't on screen.
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
