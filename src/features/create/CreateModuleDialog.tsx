/**
 * CreateModuleDialog —— 挂在 `ProjectTree` 内。
 *
 * 模块永远是当前项目的直接子级，故本表单除了名字无处可问：没有父级选择器（可能的父级
 * 只有一个）、没有颜色（无处显示它）、没有 baseUrl（那属于项目或环境）。
 */

import { useRef } from 'react';
import { useAppStore } from '@/stores/app-store';
import { CreateDialog, DescriptionField, NameField } from './CreateDialog';

export function CreateModuleDialog() {
  const draft = useAppStore((s) => s.newItem);
  const closeNewItem = useAppStore((s) => s.closeNewItem);
  const setNewItemName = useAppStore((s) => s.setNewItemName);
  const setNewItemDescription = useAppStore((s) => s.setNewItemDescription);
  const submitNewItem = useAppStore((s) => s.submitNewItem);

  const nameInputRef = useRef<HTMLInputElement>(null);
  const open = draft.open && draft.type === 'module';

  const submit = () => {
    void submitNewItem();
    closeNewItem();
  };

  return (
    <CreateDialog
      open={open}
      onClose={closeNewItem}
      title="新建模块"
      description="模块是这个项目下的一级分组"
      canSubmit={draft.name.trim().length > 0}
      onSubmit={submit}
      nameInputRef={nameInputRef}
    >
      <NameField value={draft.name} onChange={setNewItemName} inputRef={nameInputRef} />
      <DescriptionField value={draft.description} onChange={setNewItemDescription} />
    </CreateDialog>
  );
}
