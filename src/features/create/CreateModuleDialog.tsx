/**
 * CreateModuleDialog — mounted inside `ProjectTree`.
 *
 * A module is always a direct child of the active project, so this form
 * has nothing to ask beyond the name: no parent picker (there is only one
 * possible parent), no colour (nothing shows it), no baseUrl (that
 * belongs to a project or to an environment).
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
