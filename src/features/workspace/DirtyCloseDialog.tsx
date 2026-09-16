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
 * The dialog intentionally disables the backdrop close + Esc — the user
 * has to pick an explicit resolution. Closing the dialog without
 * picking would leak the dirty state (queue stays set, but the user
 * expects the action to have completed).
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
  /** When the tab is an endpoint, an accent for the title — purely cosmetic. */
  accent?: string;
}

function tabDisplay(tabId: string): TabDisplay {
  const state = useAppStore.getState();
  const tab = state.openTabs.find((t) => t.id === tabId);
  if (!tab) {
    // The tab was already closed (e.g. via another path) — the dialog
    // shouldn't be showing, but be defensive.
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
  // settings — should never be dirty, but handle gracefully.
  return { title: '设置', kindLabel: '' };
}

export function DirtyCloseDialog() {
  // Raw store reads. The dialog only renders when pendingDirtyClose is
  // non-null, so all three selectors are safe to call unconditionally.
  const batch = useAppStore((s) => s.pendingDirtyClose);
  const resolveDirtyClose = useAppStore((s) => s.resolveDirtyClose);
  // For the endpoint accent/method badge — only consulted when the
  // current tab is an endpoint. Reading the whole array keeps the
  // selector allocation-free.
  const endpoints = useAppStore((s) => s.endpoints);

  // The "current" tab in the queue. Computed off the raw state so the
  // dialog re-renders the moment `batch.index` advances (e.g. after a
  // "保存" click) without waiting for a separate store write.
  const current = useMemo(() => {
    if (!batch) return null;
    const id = batch.tabIds[batch.index];
    if (!id) return null;
    return { id, ...tabDisplay(id) };
  }, [batch]);

  if (!batch || !current) return null;

  const remaining = batch.tabIds.length - batch.index;
  const position =
    batch.tabIds.length > 1
      ? `（第 ${batch.index + 1} / ${batch.tabIds.length} 个未保存标签）`
      : '';
  const currentEndpoint =
    current.kindLabel === '接口'
      ? endpoints.find((e) => e.id === current.id) ?? null
      : null;

  return (
    <Dialog
      open
      // The store owns the lifecycle; ignore outside-close attempts so
      // the user can't lose the resolution by accident.
      onOpenChange={() => {
        /* no-op — see file header */
      }}
      disableBackdropClose
      title={
        <span className="flex items-center gap-2">
          <AlertTriangle className="h-4 w-4 shrink-0 text-amber-400" />
          <span>关闭未保存的{current.kindLabel}？</span>
        </span>
      }
      description={
        <span>
          「{current.title}」有未保存的修改。{position}
          {remaining > 1
            ? `剩余 ${remaining - 1} 个标签会按你接下来的选择处理。`
            : '请选择如何处理。'}
        </span>
      }
      footer={
        <div className="flex w-full items-center justify-between gap-2">
          <div className="text-[10px] text-muted-foreground">
            {batch.tabIds.length > 1
              ? `共 ${batch.tabIds.length} 个未保存标签等待处理`
              : '保存会写入已保存的接口数据'}
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
      {/* Body intentionally minimal — the title and footer carry the
          action. We keep one paragraph so the dialog isn't a bare shell
          when scaled up by long tab names. */}
      <div className="space-y-1 text-xs text-muted-foreground">
        <p>
          「直接关闭」会丢弃当前标签的修改，并停止询问剩余未保存标签——其他标签保持打开。
        </p>
        <p>
          「保存」会写入当前标签的修改，然后继续询问下一个未保存标签。
        </p>
        <p>
          「全部保存」会一次性保存队列中所有剩余标签的修改并关闭它们。
        </p>
        {currentEndpoint && (
          <p className="pt-1">
            <span
              className="method-badge !px-1.5 !text-[10px]"
              style={{ backgroundColor: current.accent }}
            >
              {currentEndpoint.method}
            </span>
            <span className="ml-2 text-muted-foreground/70">
              未保存的 {currentEndpoint.name} 草稿
            </span>
          </p>
        )}
      </div>
    </Dialog>
  );
}
