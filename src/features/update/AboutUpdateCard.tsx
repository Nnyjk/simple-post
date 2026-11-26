/**
 * 设置里的「关于」卡片：当前版本、更新检查、GitHub 镜像、更新方式，以及发现新版本时的入口。
 *
 * 检查结果分四类（已是最新 / 有新版本 / 检查失败 / 还没查过），文案统一由
 * `describeCheckOutcome` 给，故卡片与顶栏不会对同一次检查说两种话。
 *
 * 当前版本优先读打包时内置的那一份：它不依赖网络，也不依赖后端有没有报过版本。
 */
import { useEffect, useState } from 'react';
import { Loader2, RefreshCw, Sparkles } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Select } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { ExternalLink } from '@/components/ui/external-link';
import { useAppStore } from '@/stores/app-store';
import { APP_VERSION } from '@/lib/app-version';
import {
  GITHUB_MIRRORS,
  PROJECT_GITHUB_URL,
  mirrorGithubUrl,
  resolveGithubMirror,
} from '@/lib/github-mirror';
import { isUpdaterAvailable } from '@/lib/updater';
import { describeCheckOutcome } from '@/lib/update/messages';
import { relativeTime } from '@/lib/update/time';
import { SettingsCategory, SettingRow } from '../settings/SettingsCategory';
import { LogExportCard } from './LogExportCard';
import { UpdateDialog } from './UpdateDialog';
import { cn, focusRing } from '@/lib/utils';

export function AboutUpdateCard() {
  const info = useAppStore((s) => s.updateInfo);
  const deployment = useAppStore((s) => s.updateDeployment);
  const busy = useAppStore((s) => s.updateBusy);
  const error = useAppStore((s) => s.updateError);
  const pinned = useAppStore((s) => s.pinnedVersion);
  const autoCheck = useAppStore((s) => s.autoCheckUpdates);
  const githubMirror = useAppStore((s) => s.githubMirror);
  const checkForUpdate = useAppStore((s) => s.checkForUpdate);
  const loadDeployment = useAppStore((s) => s.loadDeployment);
  const ensurePinnedVersion = useAppStore((s) => s.ensurePinnedVersion);
  const setAutoCheckUpdates = useAppStore((s) => s.setAutoCheckUpdates);
  const setGithubMirror = useAppStore((s) => s.setGithubMirror);
  const [dialogOpen, setDialogOpen] = useState(false);

  const desktop = isUpdaterAvailable();

  // 当前版本来自运行中的应用本身，与「有没有新版」查没查成无关。
  useEffect(() => {
    void ensurePinnedVersion();
  }, [ensurePinnedVersion]);

  // 通道说明要说对「新版本从哪来」，那就得知道本部署是容器还是源码/二进制。
  useEffect(() => {
    if (desktop || deployment) return;
    void loadDeployment();
  }, [desktop, deployment, loadDeployment]);

  // 内置版本优先：「关于」页里的版本号不该取决于这次检查有没有联网。
  const currentVersion = pinned || APP_VERSION || info?.current || '';

  // web 端刷新浏览器后 `info` 是空的，而那条时刻由服务端存着（随部署形态回来），
  // 故它排在 `info` 之后作为回退 —— 刷新不该让「上次检查」退回「尚未检查」。
  const lastChecked = relativeTime(info?.checkedAt ?? deployment?.lastCheckedAt, Date.now());
  const failed = Boolean(error) || Boolean(info?.error);
  // 结果文案统一出自 `describeCheckOutcome`，卡片与顶栏不会对同一次检查说两种话。
  const outcome = error ?? (info ? describeCheckOutcome(info) : null);
  const hasUpdate = Boolean(info?.available && info.latest);

  // 有新版时这行只说时间：版本号归下方「可更新」那一行，一张卡片里不该出现两次。
  // 「已是最新版本」接在时间后面；失败原因另起一行（见下）。
  const timeText = lastChecked ? `上次检查 ${lastChecked}` : '尚未检查过更新';
  const statusLine = busy
    ? '检查中…'
    : failed || hasUpdate || !info
      ? timeText
      : `${timeText} · ${outcome ?? '已是最新版本'}`;

  const channel = desktop
    ? '桌面端 · 应用内下载并安装，安装完自动重启'
    : deployment?.shape === 'docker'
      ? '容器部署 · 拉取新镜像后重启容器'
      : '源码 / 二进制部署 · 按部署文档拉取新版本并重启';

  return (
    <>
      <SettingsCategory
        title="关于"
        description="版本、更新方式、GitHub 镜像与日志导出"
        icon={<Sparkles className="h-3.5 w-3.5" />}
        actions={
          // 右对齐的纵向小块：按钮在上，结果小字紧贴其下。
          <div className="flex flex-col items-end gap-1">
            <Button
              variant="ghost"
              size="xs"
              onClick={() => void checkForUpdate(true)}
              disabled={busy}
              className="gap-1 text-muted-foreground"
            >
              {busy ? (
                <Loader2 className="h-3 w-3 animate-spin" />
              ) : (
                <RefreshCw className="h-3 w-3" />
              )}
              检查更新
            </Button>
            <span className="flex max-w-[min(22rem,40vw)] flex-col items-end gap-0.5 text-right">
              <span
                className={cn(
                  'whitespace-nowrap text-micro',
                  failed && !busy ? 'text-destructive' : 'text-muted-foreground',
                )}
              >
                {statusLine}
              </span>
              {/* 失败原因另起一行：那句话本身较长，接在时间后面会把标题挤扁。 */}
              {failed && !busy && (
                <span className="text-micro text-destructive">{outcome ?? '检查失败'}</span>
              )}
            </span>
          </div>
        }
      >
        <SettingRow
          flush
          label="当前版本"
          description="正在运行的 Simple Post 版本"
          // 版本号只有一行，标签带第二行描述：靠顶端对齐会读成压在标签上。
          align="center"
          control={
            currentVersion ? (
              <span className="flex items-center gap-1">
                <span className="font-mono text-xs text-foreground">{currentVersion}</span>
                <ExternalLink
                  href={mirrorGithubUrl(PROJECT_GITHUB_URL, githubMirror)}
                  label="在 GitHub 上打开项目主页"
                  className={cn(
                    'inline-flex h-6 w-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-overlay-hover hover:text-foreground',
                    focusRing,
                  )}
                >
                  <GithubMark />
                </ExternalLink>
              </span>
            ) : (
              <span className="text-micro text-muted-foreground">未知</span>
            )
          }
        />
        <SettingRow
          label="自动检查更新"
          description="启动后每 24 小时最多查一次；关掉之后仍可点「检查更新」"
          control={
            <Switch
              checked={autoCheck}
              onCheckedChange={setAutoCheckUpdates}
              aria-label="自动检查更新"
            />
          }
        />
        {info?.available && info.latest && (
          // 这一行不受 `dismissedVersion` 影响：「稍后再说」只收起顶栏那枚 pill，
          // 用户主动进设置看时仍然该看到入口。
          <SettingRow
            label="可更新"
            description="有新版本可用，查看发布说明后决定何时更新"
            badge={<Badge variant="success">v{info.latest}</Badge>}
            control={
              <Button size="xs" onClick={() => setDialogOpen(true)}>
                查看更新
              </Button>
            }
          />
        )}
        <SettingRow
          label="GitHub 镜像"
          description="发布页与发布说明里的链接走所选镜像；检查更新与桌面安装包下载仍直连 GitHub"
          control={
            <Select
              aria-label="GitHub 镜像"
              value={githubMirror}
              onChange={(e) => setGithubMirror(resolveGithubMirror(e.target.value))}
              options={GITHUB_MIRRORS.map((m) => ({ value: m.id, label: m.label }))}
            />
          }
        />
        <SettingRow
          label="更新方式"
          description={channel}
          control={
            <Badge variant="muted">{desktop ? '桌面端' : (deployment?.shape ?? 'web')}</Badge>
          }
        />
      </SettingsCategory>
      {/* 日志小节自带分区头，故它与「关于」并列而不是嵌进去。 */}
      <LogExportCard />
      <UpdateDialog open={dialogOpen} onOpenChange={setDialogOpen} />
    </>
  );
}

/**
 * GitHub 的「Cat」标记。
 *
 * 用内联 SVG 而不是 `lucide-react`：1.x 起不再带品牌图标，而这里要的正是品牌本身。
 */
function GithubMark() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true" fill="currentColor" className="h-3.5 w-3.5">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0 0 16 8c0-4.42-3.58-8-8-8z" />
    </svg>
  );
}
