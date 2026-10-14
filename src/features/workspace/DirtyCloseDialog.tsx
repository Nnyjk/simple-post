/**
 * DirtyCloseDialog —— 单实例对话框，在某条关闭路径会丢掉草稿时替用户决定「这些未保存
 * 的修改怎么办」。
 *
 * 队列（`pendingDirtyClose: { tabIds, index }`）与结算动作（`resolveDirtyClose`）都归
 * store。本组件只是视图：读当前 id、查出标签的显示标题、渲染三个按钮。
 *
 * 三个按钮，按用户提需求的顺序：
 *   - 直接关闭 —— 丢弃当前标签的修改，并中止队列余下部分。批次里其他脏标签保持打开，
 *     用户不会因此丢掉不相干的工作。
 *   - 保存     —— 应用当前标签的待存修改，队列推进到下一个脏标签。若这是最后一个，
 *     对话框关闭。
 *   - 全部保存 —— 应用其余每个标签的待存修改，全部关闭，然后关掉对话框。
 *
 * 刻意禁用遮罩关闭。但标题行的 × 关闭按钮行为等同 `直接关闭`，好让用户有一条不会把
 * 队列晾在那里的显式退出路径。
 */

import { useMemo } from 'react';
import { AlertTriangle } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { Dialog } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { MethodPill } from '@/components/ui/method-pill';

interface TabDisplay {
  title: string;
  kindLabel: string;
}

function tabDisplay(tabId: string): TabDisplay {
  const state = useAppStore.getState();
  const tab = state.openTabs.find((t) => t.id === tabId);
  if (!tab) {
    return { title: '已关闭的标签', kindLabel: '' };
  }
  if (tab.kind === 'project') {
    const p = state.projects.find((x) => x.id === tabId);
    return { title: p?.name ?? '（项目已删除）', kindLabel: '项目' };
  }
  // 模块 / 集合 / 接口现在都是树节点，标签的 `kind` 就是节点的 `type`，
  // 一次查找答完三者。
  if (tab.kind === 'module' || tab.kind === 'collection' || tab.kind === 'endpoint') {
    const n = state.nodes.find((x) => x.id === tabId);
    const kindLabel = tab.kind === 'module' ? '模块' : tab.kind === 'collection' ? '集合' : '接口';
    const goneLabel =
      tab.kind === 'module'
        ? '（模块已删除）'
        : tab.kind === 'collection'
          ? '（集合已删除）'
          : '（接口已删除）';
    return { title: n?.name ?? goneLabel, kindLabel };
  }
  return { title: '设置', kindLabel: '' };
}

export function DirtyCloseDialog() {
  const batch = useAppStore((s) => s.pendingDirtyClose);
  const resolveDirtyClose = useAppStore((s) => s.resolveDirtyClose);
  const endpointDetails = useAppStore((s) => s.endpointDetails);

  const current = useMemo(() => {
    if (!batch) return null;
    const id = batch.tabIds[batch.index];
    if (!id) return null;
    return { id, ...tabDisplay(id) };
  }, [batch]);

  if (!batch || !current) return null;

  const total = batch.tabIds.length;
  const position = total > 1 ? `第 ${batch.index + 1} / ${total} 个` : null;

  // 接口专属的一行：只显示方法，不重复名字（标题已经叫出了这个标签），让正文有信息量
  // 却不啰嗦。方法存在 detail 行里。
  const currentEndpoint =
    current.kindLabel === '接口' ? (endpointDetails[current.id] ?? null) : null;

  return (
    <Dialog
      open
      // × 关闭按钮（以及 Esc）行为等同 `discard`：跳过当前标签的保存并推进。
      // 遮罩点击仍禁用 —— 不让一次误点把队列晾在那里。
      onOpenChange={(o) => {
        if (!o) resolveDirtyClose('discard');
      }}
      disableBackdropClose
      title={
        <span className="flex min-w-0 items-center gap-2">
          <AlertTriangle className="h-4 w-4 shrink-0 text-warning-foreground" />
          <span className="truncate">
            {current.kindLabel}「{current.title}」有未保存修改
          </span>
        </span>
      }
      description={position}
      footer={
        <div className="flex w-full items-center justify-between gap-2">
          <div className="text-[10px] text-muted-foreground">
            {total > 1 ? `共 ${total} 个未保存标签` : null}
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => resolveDirtyClose('discard')}
              data-testid="dirty-discard"
            >
              直接关闭
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => resolveDirtyClose('save')}
              data-testid="dirty-save"
            >
              保存
            </Button>
            <Button
              size="sm"
              onClick={() => resolveDirtyClose('saveAll')}
              data-testid="dirty-save-all"
            >
              全部保存
            </Button>
          </div>
        </div>
      }
    >
      {/* 正文刻意稀疏 —— 标题加按钮已经承载了这个决定。方法徽标是用户唯一需要的额外
          语境（按 HTTP 方法着色）。 */}
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        {currentEndpoint ? (
          <>
            <MethodPill method={currentEndpoint.method} size="sm" variant="solid" />
            <span>选择「保存」写入当前接口草稿,「直接关闭」丢弃。</span>
          </>
        ) : (
          <span>选择「保存」写入当前{current.kindLabel}修改,「直接关闭」丢弃。</span>
        )}
      </div>
    </Dialog>
  );
}
