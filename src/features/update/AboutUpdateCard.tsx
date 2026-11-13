/**
 * 设置里的「关于与更新」卡片：当前版本、上次检查、手动检查、以及「新版本怎么来」。
 *
 * 检查结果分四类（已是最新 / 有新版本 / 检查失败 / 还没查过），文案统一由
 * `describeCheckOutcome` 给，故卡片与顶栏不会对同一次检查说两种话。
 */
import { useEffect } from 'react';
import { Loader2, RefreshCw, Sparkles } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { useAppStore } from '@/stores/app-store';
import { isUpdaterAvailable } from '@/lib/updater';
import { describeCheckOutcome } from '@/lib/update/messages';
import { relativeTime } from '@/lib/update/time';
import { SettingsCategory, SettingRow } from '../settings/SettingsCategory';
import { cn } from '@/lib/utils';

export function AboutUpdateCard() {
  const info = useAppStore((s) => s.updateInfo);
  const deployment = useAppStore((s) => s.updateDeployment);
  const busy = useAppStore((s) => s.updateBusy);
  const error = useAppStore((s) => s.updateError);
  const checkForUpdate = useAppStore((s) => s.checkForUpdate);
  const loadDeployment = useAppStore((s) => s.loadDeployment);

  const desktop = isUpdaterAvailable();

  // 通道说明要说对「新版本从哪来」，那就得知道本部署是容器还是源码/二进制。
  useEffect(() => {
    if (desktop || deployment) return;
    void loadDeployment();
  }, [desktop, deployment, loadDeployment]);

  const lastChecked = info ? relativeTime(info.checkedAt, Date.now()) : null;
  const outcome = error ?? (info ? describeCheckOutcome(info) : '还没有检查过更新');
  const failed = Boolean(error) || Boolean(info?.error);

  const channel = desktop
    ? '桌面端 · 应用内下载并安装，安装完自动重启'
    : deployment?.shape === 'docker'
      ? '容器部署 · 拉取新镜像后重启容器'
      : '源码 / 二进制部署 · 按部署文档拉取新版本并重启';

  return (
    <SettingsCategory
      title="关于与更新"
      description="版本号、更新检查与更新方式"
      icon={<Sparkles className="h-3.5 w-3.5" />}
      actions={
        <Button
          variant="ghost"
          size="xs"
          onClick={() => void checkForUpdate(true)}
          disabled={busy}
          className="gap-1 text-muted-foreground"
        >
          {busy ? <Loader2 className="h-3 w-3 animate-spin" /> : <RefreshCw className="h-3 w-3" />}
          检查更新
        </Button>
      }
    >
      <SettingRow
        flush
        label="当前版本"
        description="正在运行的 Simple Post 版本"
        control={<span className="font-mono text-xs text-foreground">{info?.current || '—'}</span>}
      />
      <SettingRow
        label="上次检查"
        description={busy ? '检查中…' : outcome}
        badge={
          failed ? (
            <Badge variant="destructive">检查失败</Badge>
          ) : info?.available ? (
            <Badge variant="success">有新版本</Badge>
          ) : undefined
        }
        control={
          <span
            className={cn('text-[11px]', failed ? 'text-destructive' : 'text-muted-foreground')}
          >
            {lastChecked ?? '尚未检查'}
          </span>
        }
      />
      <SettingRow
        label="更新方式"
        description={channel}
        control={<Badge variant="muted">{desktop ? '桌面端' : (deployment?.shape ?? 'web')}</Badge>}
      />
    </SettingsCategory>
  );
}
