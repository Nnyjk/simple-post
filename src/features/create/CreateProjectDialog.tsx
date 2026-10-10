/**
 * CreateProjectDialog — the only create dialog that is not project-scoped.
 *
 * WHY IT IS MOUNTED IN `App.tsx` RATHER THAN IN A SCREEN
 * ------------------------------------------------------
 * A project is the one kind with no containing project, so it can be
 * created from three different regions: the welcome screen's card, the
 * top-bar project dropdown, and the command palette (Ctrl+K). Two of
 * those are on the *workspace* surface and one is on the *welcome*
 * surface, so mounting the dialog in either screen would leave the other
 * two entry points writing a draft nobody renders — the exact failure
 * this decomposition exists to remove.
 *
 * `App.tsx` owns the surface switch, so it owns the one portal that has
 * to outlive a switch. The module and collection dialogs do *not* need
 * this: they are only reachable from inside a project, which is exactly
 * where `ProjectTree` mounts them.
 */

import { useRef } from 'react';
import { useAppStore } from '@/stores/app-store';
import { Input } from '@/components/ui/input';
import { CreateDialog, ColorPicker, DescriptionField, Field, NameField } from './CreateDialog';

export function CreateProjectDialog() {
  const draft = useAppStore((s) => s.newItem);
  const closeNewItem = useAppStore((s) => s.closeNewItem);
  const setNewItemName = useAppStore((s) => s.setNewItemName);
  const setNewItemDescription = useAppStore((s) => s.setNewItemDescription);
  const setNewItemBaseUrl = useAppStore((s) => s.setNewItemBaseUrl);
  const setNewItemColor = useAppStore((s) => s.setNewItemColor);
  const submitNewItem = useAppStore((s) => s.submitNewItem);

  const nameInputRef = useRef<HTMLInputElement>(null);
  const open = draft.open && draft.type === 'project';

  // Empty is allowed here, unlike the other two: `submitNewItem` falls
  // back to 「新项目」, and a first-run user who just wants *a* project
  // should not have to name it before they can see it.
  const submit = () => {
    void submitNewItem();
    closeNewItem();
  };

  return (
    <CreateDialog
      open={open}
      onClose={closeNewItem}
      title="新建项目"
      description="项目是模块、集合与接口的容器"
      canSubmit
      onSubmit={submit}
      nameInputRef={nameInputRef}
      wide
    >
      <NameField
        value={draft.name}
        onChange={setNewItemName}
        inputRef={nameInputRef}
        placeholder="输入名称"
      />
      <DescriptionField value={draft.description} onChange={setNewItemDescription} />
      <Field label="Base URL">
        <Input
          value={draft.baseUrl}
          onChange={(e) => setNewItemBaseUrl(e.target.value)}
          placeholder="https://api.example.com"
        />
      </Field>
      <Field label="颜色">
        <ColorPicker value={draft.color} onChange={setNewItemColor} />
      </Field>
    </CreateDialog>
  );
}
