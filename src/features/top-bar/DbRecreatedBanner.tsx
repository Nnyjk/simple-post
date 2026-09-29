/**
 * "Your workspace was reset" banner.
 *
 * Simple Post has no database migration path. When `db::init` finds a
 * `store.db` written by an older layout it deletes it and starts over —
 * so the data really is gone, and the user has to be told.
 *
 * Without this, a reset workspace is indistinguishable from one that was
 * always empty: same blank tree, same "create your first project" pitch,
 * no indication anything was lost. That is the worst possible way to
 * discover your work was deleted.
 *
 * Deliberately a separate component from `BootstrapErrorBanner` rather
 * than a reuse of it. A reset workspace loaded perfectly — showing
 * "启动加载失败" over it would be a lie, and the retry button would
 * re-run a load that already succeeded.
 */
import { AlertTriangle, X } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

export function DbRecreatedBanner() {
  const dbRecreatedAt = useAppStore((s) => s.dbRecreatedAt);
  const dismissDbRecreated = useAppStore((s) => s.dismissDbRecreated);

  if (dbRecreatedAt === null) return null;

  const at = new Date(dbRecreatedAt);
  const stamp = Number.isNaN(at.getTime()) ? '' : `（${at.toLocaleString()}）`;

  return (
    <div
      role="alert"
      aria-live="assertive"
      data-testid="db-recreated"
      className="flex shrink-0 items-center gap-2 border-b border-warning/40 bg-warning/10 px-3 py-1.5 text-xs text-warning"
    >
      <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
      <span className="font-medium">工作区已重置</span>
      <span className="min-w-0 flex-1 truncate opacity-90">
        本地数据库结构已升级，旧数据无法迁移而被清空{stamp}
        。当前版本不再播种示例数据，请重新导入或创建项目。
      </span>
      <Button
        variant="ghost"
        size="icon"
        onClick={dismissDbRecreated}
        aria-label="忽略重置提示"
        className={cn('h-6 w-6 text-warning hover:bg-warning/15 hover:text-warning')}
      >
        <X className="h-3.5 w-3.5" />
      </Button>
    </div>
  );
}
