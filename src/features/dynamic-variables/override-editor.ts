/**
 * 覆盖值编辑器能不能开、开了之后框里该是哪份文本。
 *
 * 抽成不依赖 React 的模块是为了能直接单测：这个判定同时挡住两条入口（「编辑」按钮与
 * 「当前值」那行摘要），错一处就会把上一个环境的值写进这一格。
 */
import { dynamicListMatchesEnvironment } from '@/stores/dynamic-slice';

export interface OverrideEditor {
  /** 此刻能不能照着列表里的「当前值」写覆盖值。 */
  editable: boolean;
  /** 打开时框里的初值。不可编辑时也照给，方便调用方无脑 `setDraft`。 */
  initialValue: string;
}

/**
 * 覆盖值编辑器的初始状态。
 *
 * 判据是「列表里那批当前值是不是当前这个 (项目 × 取数环境) 拉到的」：切换环境的那一刻列表
 * 还在路上，内容仍是上一个环境的——此刻开编辑器，框里是 A 的值，保存却会写进 B。
 */
export function overrideEditor(
  listScope: string | null,
  projectId: string,
  environmentId: string | null,
  currentValue: string | null | undefined,
): OverrideEditor {
  return {
    editable: dynamicListMatchesEnvironment(listScope, projectId, environmentId),
    initialValue: currentValue ?? '',
  };
}
