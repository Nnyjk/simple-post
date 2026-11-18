/**
 * 更新对话框 —— 版本对照、发布说明、以及按宿主分流的主操作。
 *
 * 三条通道的差别不在长相，而在「点下去以后谁去搬新版本」：
 * - 桌面壳（`IS_TAURI_SHELL`）：**一键** —— 下载安装包、安装器接管、自动安装、自动重启。
 *   `tauri dev` 没有签名产物，updater 跑不了，但按钮仍然是「下载并安装」，只是禁用并说明
 *   原因；桌面里绝不退回「去 GitHub 下载」，那不是这条链路，也完全偏离一键的承诺。
 * - 容器（`serve-web` 跑在镜像里）：只有运维在部署侧开了就地执行才有「立即执行更新」；
 *   没开时页脚只剩「稍后再说」，正文给 `docker pull` 命令与发布页链接。
 * - 源码 / 二进制：没有可代跑的东西，正文给命令与发布页链接，页脚同样没有动作。
 *
 * 浏览器里没有可安装的进程，页脚放一个「去下载」只会被读成一键更新 —— 所以除了桌面壳与
 * 开了就地执行的容器，页脚不给任何主操作。
 */
import { useEffect, useRef, useState } from 'react';
import { ArrowRight, Check, Copy, ExternalLink } from 'lucide-react';
import { Dialog } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { useAppStore } from '@/stores/app-store';
import type { UpdateApplyResult } from '@/types/update';
import { isUpdaterAvailable } from '@/lib/updater';
import { IS_TAURI_SHELL } from '@/lib/tauri-commands';
import { mirrorGithubUrl } from '@/lib/github-mirror';
import { formatDate, relativeTime } from '@/lib/update/time';
import { cn } from '@/lib/utils';
import { ReleaseNotes } from './ReleaseNotes-lazy';

interface UpdateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/** 复制按钮的成功态停留时长。与设置里「接入配置」那一格同一种反馈节奏。 */
const COPY_RESET_MS = 2000;

/**
 * 命令行块 —— 原样展示、等宽、可复制。
 *
 * 命令来自 `update_deployment.command`（后端按部署形态拼），界面不改写它：用户要复制的是
 * 那条真能跑的命令，不是被本地化过的一行字。
 */
function CommandLine({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);
  const timerRef = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (timerRef.current !== null) window.clearTimeout(timerRef.current);
    },
    [],
  );

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      if (timerRef.current !== null) window.clearTimeout(timerRef.current);
      timerRef.current = window.setTimeout(() => {
        setCopied(false);
        timerRef.current = null;
      }, COPY_RESET_MS);
    } catch {
      // 剪贴板不可用（权限被拒、非安全上下文）时保持原样：用户仍能选中 <pre> 手动复制。
    }
  };

  return (
    <div className="flex items-start gap-2">
      <pre className="min-w-0 flex-1 overflow-x-auto rounded-md border border-border bg-muted/60 px-2 py-1.5 font-mono text-micro leading-relaxed">
        {command}
      </pre>
      <Button
        variant={copied ? 'secondary' : 'outline'}
        size="xs"
        onClick={copy}
        className="shrink-0 gap-1"
        aria-label="复制命令"
      >
        {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
        {copied ? '已复制' : '复制'}
      </Button>
    </div>
  );
}

/** 进度条。`percent` 为 `null` 表示总量未知（服务端没给 contentLength），显示不确定态。 */
function DownloadProgress({
  percent,
  phase,
}: {
  percent: number | null;
  phase: 'download' | 'install';
}) {
  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between text-micro text-muted-foreground">
        <span>{phase === 'install' ? '正在安装…' : '正在下载更新包'}</span>
        {phase === 'download' && percent !== null && <span className="font-mono">{percent}%</span>}
      </div>
      <div
        role="progressbar"
        aria-label={phase === 'install' ? '正在安装' : '正在下载更新包'}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={phase === 'download' && percent !== null ? percent : undefined}
        className="h-1.5 overflow-hidden rounded-full bg-muted"
      >
        {phase === 'install' || percent === null ? (
          // 总量未知时不假装知道还剩多少：一条脉动的窄条比一个停在 0% 的满条诚实。
          <div className="h-full w-1/3 animate-pulse rounded-full bg-primary" />
        ) : (
          <div
            className="h-full rounded-full bg-primary transition-[width] duration-200"
            style={{ width: `${percent}%` }}
          />
        )}
      </div>
    </div>
  );
}

/** 执行结果。`exitCode` 非 0 是这一块唯一需要抢眼的事，故整块换成破坏色。 */
function ApplyResultBlock({ result }: { result: UpdateApplyResult | null }) {
  if (!result) return null;
  const failed = result.exitCode !== 0;
  const output = [result.stdout.trim(), result.stderr.trim()].filter(Boolean).join('\n');
  return (
    <section
      className={cn(
        'space-y-1.5 rounded-md border px-2.5 py-2',
        failed ? 'border-destructive/50 bg-destructive/10' : 'border-border/70 bg-background/40',
      )}
    >
      <div
        className={cn('text-micro font-medium', failed ? 'text-destructive' : 'text-foreground')}
      >
        {failed ? `更新命令失败（退出码 ${result.exitCode}）` : '更新命令已执行'}
      </div>
      <pre className="max-h-40 overflow-auto whitespace-pre-wrap rounded bg-muted/50 px-2 py-1.5 font-mono text-nano leading-relaxed">
        {output || '（没有输出）'}
      </pre>
      {!failed && <p className="text-micro text-muted-foreground">更新已下发，重启完成后生效。</p>}
    </section>
  );
}

export function UpdateDialog({ open, onOpenChange }: UpdateDialogProps) {
  const info = useAppStore((s) => s.updateInfo);
  const deployment = useAppStore((s) => s.updateDeployment);
  const busy = useAppStore((s) => s.updateBusy);
  const error = useAppStore((s) => s.updateError);
  const releaseDate = useAppStore((s) => s.updateReleaseDate);
  const progress = useAppStore((s) => s.updateProgress);
  const phase = useAppStore((s) => s.updatePhase);
  const applyResult = useAppStore((s) => s.applyResult);
  const loadDeployment = useAppStore((s) => s.loadDeployment);
  const applyUpdate = useAppStore((s) => s.applyUpdate);
  const dismissUpdate = useAppStore((s) => s.dismissUpdate);
  const runDesktopInstall = useAppStore((s) => s.runDesktopInstall);
  const mirror = useAppStore((s) => s.githubMirror);

  // 桌面壳是「一键更新」唯一的宿主；`updaterReady` 另算，因为 `tauri dev` 里壳还在、
  // 签名产物却不在，那一种是「按钮禁用 + 说明」，不是「换一条更新通道」。
  const shell = IS_TAURI_SHELL;
  const updaterReady = isUpdaterAvailable();

  // 部署形态只在需要它的那条通道上问：桌面壳没有 docker / binary 之分。
  useEffect(() => {
    if (!open || shell || deployment) return;
    void loadDeployment();
  }, [open, shell, deployment, loadDeployment]);

  const latest = info?.latest ?? null;
  const published = formatDate(releaseDate);
  const checked = info ? relativeTime(info.checkedAt, Date.now()) : null;
  const installing = busy && phase !== null;

  const dismiss = () => {
    if (latest) dismissUpdate(latest);
    onOpenChange(false);
  };

  const docker = !shell && deployment?.shape === 'docker';
  const binary = !shell && !docker;

  const primary = (() => {
    // 桌面壳：一键。壳在但 updater 不可用（`tauri dev`）时按钮保留、禁用、讲清原因，
    // 因为这里换任何别的通道都违背「点了就自动装完重启」这一条承诺。
    if (shell) {
      return (
        <div className="flex flex-col items-end gap-1">
          <Button
            size="sm"
            onClick={() => void runDesktopInstall()}
            disabled={!updaterReady || busy}
            title={
              updaterReady ? undefined : '开发构建不含签名产物：应用内一键更新只在打包版本可用'
            }
            className="gap-1.5"
          >
            {installing ? '正在安装…' : '下载并安装'}
          </Button>
          <span className="text-micro text-muted-foreground">
            {updaterReady
              ? '安装包在后台静默运行，装完自动重启，不需要卸载或再点下一步。'
              : '打包版本里点这里会下载安装包、自动安装并重启。'}
          </span>
        </div>
      );
    }
    // 容器：只有运维在部署侧打开了就地更新，这里才能真正替用户执行。
    if (docker && deployment?.applyEnabled === true) {
      return (
        <Button size="sm" onClick={() => void applyUpdate()} disabled={busy} className="gap-1.5">
          {busy ? '正在执行…' : '立即执行更新'}
        </Button>
      );
    }
    // 浏览器里没有可安装的进程：页脚不给动作，说明与发布页链接都在正文里。
    return null;
  })();

  // 高度受限的 flex 列：页头与页脚不参与压缩（`shrink-0` 在 `Dialog` 里），正文吃剩下的
  // 高度并自己滚。只给正文 `max-h-[62vh]` 的话，页头 + 62vh + 页脚会超过 86vh 的面板，
  // 发布说明一长就把页脚按钮推出视口。
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={latest ? `发现新版本 v${latest}` : '更新'}
      description="查看这个版本带来什么，再决定什么时候更新"
      className="flex w-[560px] max-w-[92vw] max-h-[86vh] flex-col"
      bodyClassName="min-h-0 flex-1 overflow-y-auto overscroll-contain"
      footer={
        <>
          <Button variant="ghost" size="sm" onClick={dismiss}>
            稍后再说
          </Button>
          {primary}
        </>
      }
    >
      <div className="space-y-3">
        {/* 版本对照：等宽，因为版本号是一个一个字符读的。 */}
        <div className="flex items-center gap-2 text-xs">
          <span className="font-mono text-muted-foreground">{info?.current || '—'}</span>
          <ArrowRight className="h-3 w-3 shrink-0 text-muted-foreground" />
          <span className="font-mono text-sm font-semibold text-foreground">{latest ?? '—'}</span>
          <span className="ml-auto flex items-center gap-2 text-micro text-muted-foreground">
            {published && <span>发布于 {published}</span>}
            {checked && <span>检查于 {checked}</span>}
          </span>
        </div>

        {info?.url && (
          <a
            href={mirrorGithubUrl(info.url, mirror)}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-1 text-micro text-primary underline-offset-2 hover:underline"
          >
            在 GitHub 查看
            <ExternalLink className="h-3 w-3" />
          </a>
        )}

        {/* 发布说明 = 这个版本的新特性。空说明不空着，写清楚它没填。 */}
        <section className="rounded-md border border-border/70 bg-background/40 p-3">
          <div className="mb-1.5 text-micro font-medium text-muted-foreground">发布说明</div>
          {info?.notes ? (
            <ReleaseNotes notes={info.notes} mirror={mirror} />
          ) : (
            <p className="text-micro text-muted-foreground">该版本未填写发布说明</p>
          )}
        </section>

        {!shell && (
          <section className="space-y-2">
            {deployment?.command && <CommandLine command={deployment.command} />}
            {binary ? (
              <p className="text-micro leading-relaxed text-muted-foreground">
                当前以源码 / 二进制方式运行：按部署文档拉取新版本并重启；新版本的发布页在上面的「在
                GitHub 查看」里。
              </p>
            ) : (
              <p className="text-micro leading-relaxed text-muted-foreground">
                {deployment?.applyEnabled
                  ? '点「立即执行更新」会在容器内执行运维声明的更新命令；也可以自己在仓库根目录执行上面的命令。'
                  : '当前部署没有开启就地更新：请到部署侧的仓库根目录执行上面的命令，或按文档拉取新镜像并重启。'}
              </p>
            )}
          </section>
        )}

        {installing && <DownloadProgress percent={progress} phase={phase ?? 'download'} />}

        <ApplyResultBlock result={applyResult} />

        {error && (
          <p className="rounded-md border border-destructive/40 bg-destructive/10 px-2 py-1.5 text-micro text-destructive">
            {error}
          </p>
        )}
      </div>
    </Dialog>
  );
}
