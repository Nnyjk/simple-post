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
import { CreateDialog, ColorPicker, DescriptionField, Field, NameField } from './CreateDialog';

/**
 * The rows `projects_create` seeds. Mirrors `DEFAULT_ENVIRONMENT_NAME` and
 * `DEFAULT_MODULE_NAME` in `crates/simple-post-core/src/db/projects.rs`.
 *
 * Duplicated rather than fetched, for the same reason
 * `lib/palette.ts` mirrors `domain::default_project_color()`: the backend
 * is the single source of truth and this only feeds the hint under the
 * field. The response to `projects_create` carries the real values, so a
 * drift here shows up as a wrong hint and nothing else — change both
 * sides together.
 *
 * There is deliberately **no** base-URL hint, because a new project seeds
 * no service at all: `defaultBaseUrlDefinitionId` comes back `null` and
 * the user adds services, stars one, and fills each module's address in
 * Settings. Asking for a URL here would be asking for something that
 * nothing stores — the field it used to bind to was dropped along with
 * the default service.
 */
const DEFAULT_ENVIRONMENT_NAME = '默认环境';
const DEFAULT_MODULE_NAME = '默认模块';

export function CreateProjectDialog() {
  const draft = useAppStore((s) => s.newItem);
  const closeNewItem = useAppStore((s) => s.closeNewItem);
  const setNewItemName = useAppStore((s) => s.setNewItemName);
  const setNewItemDescription = useAppStore((s) => s.setNewItemDescription);
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
    >
      <NameField
        value={draft.name}
        onChange={setNewItemName}
        inputRef={nameInputRef}
        placeholder="输入名称"
      />
      <DescriptionField value={draft.description} onChange={setNewItemDescription} />
      <Field label="颜色">
        <ColorPicker value={draft.color} onChange={setNewItemColor} />
      </Field>
      <p className="text-[11px] leading-relaxed text-muted-foreground">
        新项目会带一个环境「{DEFAULT_ENVIRONMENT_NAME}
        」和一个模块「{DEFAULT_MODULE_NAME}
        」，都可以就地改名。服务清单与各模块的前置 URL 在「设置 · 环境变量」里配置。
      </p>
    </CreateDialog>
  );
}
