/**
 * 浮层层级。数字越大越靠上，从下到上四层各管一件事：`popover`（下拉、地址来源、变量
 * 提示）、`contextMenu`（右键菜单）、`tooltip`（悬浮提示）、`modal`（模态对话框与命令
 * 面板）。
 *
 * 模态在最高层：模态打开时背景不可操作，背景的提示气泡若浮在遮罩之上会被读成「遮罩没
 * 盖住」。模态里的提示气泡是另一层（`tooltipInModal`，110）：气泡 portal 到 body，停在
 * 模态之下会被遮罩盖住。`Tooltip` 按触发元素是否落在 `[data-modal-surface]` 里选层。
 *
 * 组件里手写 `z-[200]` 或内联 `zIndex` 就是自己另立一层，层级只从本文件取。
 */

export const LAYER = {
  popover: 'z-50',
  contextMenu: 'z-[60]',
  tooltip: 'z-[70]',
  modal: 'z-[100]',
  /** 模态面板**内部**的提示气泡。它在模态之上，否则会被面板盖住。 */
  tooltipInModal: 'z-[110]',
} as const;
