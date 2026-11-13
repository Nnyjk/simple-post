/**
 * 更新对话框 —— 版本对照、发布说明、以及按宿主分流的主操作。
 *
 * 三条通道的差别不在长相，而在「点下去以后谁去搬新版本」：
 * - 桌面壳：updater 插件下载安装包并在完成后重启进程，界面只报进度。
 * - 容器（`serve-web` 跑在镜像里）：更新由平台侧下发，这里能给的只有 `docker pull`
 *   那一条命令；服务端允许就地执行时才能点「立即执行更新」。
 * - 源码 / 二进制：没有可代跑的东西，说明文字 + 命令，用户按部署文档来。
 *
 * 主操作按宿主分流是刻意的：桌面上不可能执行 docker 命令，容器里也没有 updater 插件。
 */
import { useEffect, useRef, useState } from 'react';
import { ArrowRight, Check, Copy, ExternalLink } from 'lucide-react';
import { Dialog } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { useAppStore } from '@/stores/app-store';
import type { UpdateApplyResult } from '@/types/update';
import { isUpdaterAvailable } from '@/lib/updater';
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
      <pre className="min-w-0 flex-1 overflow-x-auto rounded-md border border-border bg-muted/60 px-2 py-1.5 font-mono text-[11px] leading-relaxed">
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
      <div className="flex items-center justify-between text-[11px] text-muted-foreground">
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
        className={cn('text-[11px] font-medium', failed ? 'text-destructive' : 'text-foreground')}
      >
        {failed ? `更新命令失败（退出码 ${result.exitCode}）` : '更新命令已执行'}
      </div>
      <pre className="max-h-40 overflow-auto whitespace-pre-wrap rounded bg-muted/50 px-2 py-1.5 font-mono text-[10px] leading-relaxed">
        {output || '（没有输出）'}
      </pre>
      {!failed && <p className="text-[11px] text-muted-foreground">更新已下发，重启完成后生效。</p>}
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

  const desktop = isUpdaterAvailable();

  // 部署形态只在需要它的那条通道上问：桌面壳没有 docker / binary 之分。
  useEffect(() => {
    if (!open || desktop || deployment) return;
    void loadDeployment();
  }, [open, desktop, deployment, loadDeployment]);

  const latest = info?.latest ?? null;
  const published = formatDate(releaseDate);
  const checked = info ? relativeTime(info.checkedAt, Date.now()) : null;
  const installing = busy && phase !== null;

  const dismiss = () => {
    if (latest) dismissUpdate(latest);
    onOpenChange(false);
  };

  const docker = !desktop && deployment?.shape === 'docker';
  const binary = !desktop && !docker;

  const primary = (() => {
    if (desktop) {
      return (
        <Button
          size="sm"
          onClick={() => void runDesktopInstall()}
          disabled={busy}
          className="gap-1.5"
        >
          {installing ? '正在安装…' : '下载并安装'}
        </Button>
      );
    }
    if (docker) {
      if (deployment?.applyEnabled !== true) return null;
      return (
        <Button size="sm" onClick={() => void applyUpdate()} disabled={busy} className="gap-1.5">
          {busy ? '正在执行…' : '立即执行更新'}
        </Button>
      );
    }
    return null;
  })();

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={latest ? `发现新版本 v${latest}` : '更新'}
      description="查看这个版本带来什么，再决定什么时候更新"
      className="w-[560px] max-w-[92vw]"
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
          <span className="ml-auto flex items-center gap-2 text-[11px] text-muted-foreground">
            {published && <span>发布于 {published}</span>}
            {checked && <span>检查于 {checked}</span>}
          </span>
        </div>

        {info?.url && (
          <a
            href={info.url}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-1 text-[11px] text-primary underline-offset-2 hover:underline"
          >
            在 GitHub 查看
            <ExternalLink className="h-3 w-3" />
          </a>
        )}

        {/* 发布说明 = 这个版本的新特性。空说明不空着，写清楚它没填。 */}
        <section className="rounded-md border border-border/70 bg-background/40 p-3">
          <div className="mb-1.5 text-[11px] font-medium text-muted-foreground">发布说明</div>
          {info?.notes ? (
            <ReleaseNotes notes={info.notes} />
          ) : (
            <p className="text-[11px] text-muted-foreground">该版本未填写发布说明</p>
          )}
        </section>

        {!desktop && (
          <section className="space-y-2">
            {deployment?.command && <CommandLine command={deployment.command} />}
            {binary ? (
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                当前以源码 / 二进制方式运行，请按部署文档拉取新版本并重启。
              </p>
            ) : (
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                {deployment?.applyEnabled
                  ? '点「立即执行更新」会在服务端拉取新镜像并下发重启；也可以自己执行上面的命令。'
                  : '当前部署没有开启就地更新：请到部署侧执行上面的命令，或按文档拉取新镜像并重启。'}
              </p>
            )}
          </section>
        )}

        {installing && <DownloadProgress percent={progress} phase={phase ?? 'download'} />}

        <ApplyResultBlock result={applyResult} />

        {error && (
          <p className="rounded-md border border-destructive/40 bg-destructive/10 px-2 py-1.5 text-[11px] text-destructive">
            {error}
          </p>
        )}
      </div>
    </Dialog>
  );
}
