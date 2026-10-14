/**
 * CreateProjectDialog —— 唯一一个不隶属于项目的创建对话框。
 *
 * 项目是唯一没有上级项目的类型，可从三处创建：欢迎页卡片、顶栏项目下拉、命令面板
 * （Ctrl+K）。其中两处在 *workspace* 面、一处在 *welcome* 面，挂在任一屏都会让另两个
 * 入口写出一个没人渲染的草稿。故这个 portal 挂在 `App.tsx` —— 门面切换由它掌管，只有它
 * 能让 portal 活过切换。模块与集合对话框不需要：它们只在项目内可达，那正是 `ProjectTree`
 * 挂载它们的地方。
 */

import { useRef } from 'react';
import { useAppStore } from '@/stores/app-store';
import { CreateDialog, ColorPicker, DescriptionField, Field, NameField } from './CreateDialog';

/**
 * `projects_create` 播种的行。对应 `crates/simple-post-core/src/db/projects.rs` 里的
 * `DEFAULT_ENVIRONMENT_NAME` 与 `DEFAULT_MODULE_NAME`。
 *
 * 刻意复制而非拉取，理由同 `lib/palette.ts` 镜像 `domain::default_project_color()`：
 * 后端是唯一真相源，这份只喂字段下方的提示；`projects_create` 的响应带着真值，故这里漂移
 * 最多表现为提示写错，不会波及数据 —— 两边一起改。
 *
 * 刻意**不**给 baseUrl 提示：新项目不播种任何服务，`defaultBaseUrlDefinitionId` 回来是
 * `null`，服务由用户自己添加、打星号、再在设置里逐模块填地址。
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

  // 这里允许留空，另两个对话框不允许：`submitNewItem` 会回落到「新项目」，首次使用的
  // 用户只想先有个项目，不该非得先起名才看得见它。
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
