/**
 * DirtyCloseDialog — single-instance dialog that mediates "what should we
 * do about unsaved edits?" when a close path would otherwise drop a draft.
 *
 * The store owns the queue (`pendingDirtyClose: { tabIds, index }`) and
 * the resolution action (`resolveDirtyClose`). This component is just
 * the view: it reads the current id, looks up the tab's display title,
 * and renders the three-button chrome.
 *
 * Three buttons, in the order the user spec calls out:
 *   - 直接关闭 — drop the current tab's changes; abort the rest of the
 *     queue. Other dirty tabs in the batch stay open so the user
 *     doesn't lose unrelated work.
 *   - 保存     — apply the current tab's pending edits and advance to
 *     the next dirty tab in the queue. If this was the last one, the
 *     dialog closes.
 *   - 全部保存 — apply every remaining tab's pending edits, close them
 *     all, then close the dialog.
 *
 * The dialog intentionally disables the backdrop close. The × close
 * button in the title row, however, behaves like `直接关闭` — that
 * way the user has an explicit dismiss path that doesn't strand the
 * queue (previously × was a no-op and effectively dead).
 */

import { useMemo } from 'react';
import { AlertTriangle } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { Dialog } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { methodColorVar } from '@/lib/utils';

interface TabDisplay {
  title: string;
  kindLabel: string;
  /** When the tab is an endpoint, an accent for the method badge. */
  accent?: string;
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
  if (tab.kind === 'module') {
    const m = state.modules.find((x) => x.id === tabId);
    return { title: m?.name ?? '（模块已删除）', kindLabel: '模块' };
  }
  if (tab.kind === 'collection') {
    const c = state.collections.find((x) => x.id === tabId);
    return { title: c?.name ?? '（集合已删除）', kindLabel: '集合' };
  }
  if (tab.kind === 'endpoint') {
    const e = state.endpoints.find((x) => x.id === tabId);
    return {
      title: e?.name ?? '（接口已删除）',
      kindLabel: '接口',
      accent: e ? methodColorVar(e.method) : undefined,
    };
  }
  return { title: '设置', kindLabel: '' };
}

export function DirtyCloseDialog() {
  const batch = useAppStore((s) => s.pendingDirtyClose);
  const resolveDirtyClose = useAppStore((s) => s.resolveDirtyClose);
  const endpoints = useAppStore((s) => s.endpoints);

  const current = useMemo(() => {
    if (!batch) return null;
    const id = batch.tabIds[batch.index];
    if (!id) return null;
    return { id, ...tabDisplay(id) };
  }, [batch]);

  if (!batch || !current) return null;

  const total = batch.tabIds.length;
  const position = total > 1 ? `第 ${batch.index + 1} / ${total} 个` : null;

  // Endpoint-specific row: shows the method without re-stating the name
  // (the title already names the tab). Keeps the body visually useful
  // without being repetitive.
  const currentEndpoint =
    current.kindLabel === '接口'
      ? endpoints.find((e) => e.id === current.id) ?? null
      : null;

  return (
    <Dialog
      open
      // The × close button (and Escape) behave like `discard`: skip
      // the current tab's save and advance. Backdrop click is still
      // disabled (don't let an accidental click strand the queue).
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
      {/* Body is intentionally sparse — the title + buttons carry the
          decision. The method badge is the only extra context the
          user needs (color-coded by HTTP method). */}
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        {currentEndpoint ? (
          <>
            <span
              className="method-badge !px-1.5 !text-[10px]"
              style={{ backgroundColor: current.accent }}
            >
              {currentEndpoint.method}
            </span>
            <span>选择「保存」写入当前接口草稿,「直接关闭」丢弃。</span>
          </>
        ) : (
          <span>
            选择「保存」写入当前{current.kindLabel}修改,「直接关闭」丢弃。
          </span>
        )}
      </div>
    </Dialog>
  );
}