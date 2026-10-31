import { useCallback, useMemo, useRef, useState } from 'react';
import { Play, Square } from 'lucide-react';
import { Dialog } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { useAppStore } from '@/stores/app-store';
import { enabledDynamicNameSet } from '@/features/http-tester/model/dynamicVars';
import { collectSubtreeEndpoints, countSubtree } from './model/collect';
import { freezeEnvironments, sendBatchItem, type BatchSendContext } from './model/send';
import { runBatch } from './model/runner';
import type { BatchItem, BatchOutcome, BatchReport } from './model/types';

/**
 * 集合批量运行的对话框。
 *
 * 一件事：把某个容器（集合或模块）下面的接口按树的显示顺序逐个发出去，然后报一份
 * 汇总。发出去的每一条都落进响应历史，事后可以在接口自己的历史面板里复核。
 *
 * **顺序执行，不并发**：环境变量的写回由脚本产生，并发时写回的先后不可预期，报告也
 * 就不可复现。顺序执行时「第几条看到的是哪一份变量」是确定的。
 *
 * **环境快照**：批次开始时把环境复制一份，整批都用这份副本。脚本写回的变量仍然落库
 * （后端做），但不影响这一批后面那几条看到的值。跑完之后再把写回同步进本地副本。
 */
export function BatchRunDialog({
  open,
  onOpenChange,
  rootId,
  rootName,
  rootKind,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 被运行的容器（集合或模块）的节点 id。 */
  rootId: string;
  /** 它的名字，标题里显示。 */
  rootName: string;
  /** 它是集合还是模块 —— 标题与措辞按它分。 */
  rootKind: 'collection' | 'module';
}) {
  // store 的原始引用。逐个 `useAppStore` 取用，保持 Zustand v5「selector 不新建对象」
  // 规则，派生列表一律在本地 `useMemo` 里算。
  const nodes = useAppStore((s) => s.nodes);
  const endpointDetails = useAppStore((s) => s.endpointDetails);
  const environments = useAppStore((s) => s.environments);
  const projects = useAppStore((s) => s.projects);
  const requestSettings = useAppStore((s) => s.requestSettings);
  const activeEnvironmentId = useAppStore((s) => s.activeEnvironmentId);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const dynamicVariables = useAppStore((s) => s.dynamicVariables);
  const resolveDynamicVariables = useAppStore((s) => s.resolveDynamicVariables);
  const addResponseHistory = useAppStore((s) => s.addResponseHistory);
  const applyEnvWrites = useAppStore((s) => s.applyEnvWrites);

  /** 正在跑的是哪一条。`null` = 没在跑。 */
  const [runningIndex, setRunningIndex] = useState<number | null>(null);
  /** 每一条的结论。`null` = 还没跑完。 */
  const [outcomes, setOutcomes] = useState<(BatchOutcome | null)[]>([]);
  /** 跑完之后的汇总。没跑完是 `null`。 */
  const [report, setReport] = useState<BatchReport | null>(null);
  /** 中止信号。 */
  const abortRef = useRef<AbortController | null>(null);

  const endpointNodes = useMemo(() => collectSubtreeEndpoints(nodes, rootId), [nodes, rootId]);
  const counts = useMemo(() => countSubtree(nodes, rootId), [nodes, rootId]);
  const items = useMemo<BatchItem[]>(
    () =>
      endpointNodes.map((n) => {
        const detail = endpointDetails[n.id];
        return {
          endpointId: n.id,
          name: n.name,
          method: detail?.method ?? 'GET',
          url: detail?.url ?? '',
        };
      }),
    [endpointNodes, endpointDetails],
  );
  // 变量候选只认当前项目里的定义：别的项目的同名变量不是同一个东西。
  const enabledDynamicNames = useMemo(
    () => enabledDynamicNameSet(dynamicVariables.filter((v) => v.projectId === activeProjectId)),
    [dynamicVariables, activeProjectId],
  );

  const running = runningIndex !== null;

  const run = useCallback(async () => {
    if (items.length === 0) return;
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    setRunningIndex(-1);
    setOutcomes(new Array(items.length).fill(null));
    setReport(null);

    // 环境快照：整批都用这一份，脚本写回的变量不会渗进后面那几条。
    const ctx: BatchSendContext = {
      nodes,
      endpointDetails,
      environments: freezeEnvironments(environments),
      projects,
      activeEnvironmentId,
      activeProjectId,
      settings: requestSettings,
      enabledDynamicNames,
      resolveDynamicVariables,
      addResponseHistory,
    };

    // 脚本写回的环境变量攒到整批跑完再同步进本地副本：中途同步会让后面那几条看到
    // 被改过的变量，快照隔离就破了。
    const writes = new Map<string, { key: string; value: string | null }[]>();

    const result = await runBatch(items, (item, signal) => sendBatchItem(item, ctx, signal), {
      signal: ctrl.signal,
      onStart: (index) => setRunningIndex(index),
      onOutcome: (outcome, index) => {
        setOutcomes((prev) => {
          const next = prev.slice();
          next[index] = outcome;
          return next;
        });
        if (outcome.envWrites && outcome.envWrites.length > 0 && outcome.envId) {
          const bucket = writes.get(outcome.envId) ?? [];
          bucket.push(...outcome.envWrites);
          writes.set(outcome.envId, bucket);
        }
      },
    });

    abortRef.current = null;
    setRunningIndex(null);
    setReport(result.report);
    for (const [envId, envWrites] of writes) {
      applyEnvWrites(envId, envWrites);
    }
  }, [
    items,
    nodes,
    endpointDetails,
    environments,
    projects,
    activeEnvironmentId,
    activeProjectId,
    requestSettings,
    enabledDynamicNames,
    resolveDynamicVariables,
    addResponseHistory,
    applyEnvWrites,
  ]);

  const abort = useCallback(() => {
    abortRef.current?.abort();
  }, []);

  const kindLabel = rootKind === 'collection' ? '集合' : '模块';
  const scopeText =
    counts.collections > 0
      ? `含 ${counts.collections} 个子集合、${counts.endpoints} 个接口`
      : `含 ${counts.endpoints} 个接口`;

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={`批量运行 · ${rootName}`}
      description={`${kindLabel}「${rootName}」${scopeText}，按树的显示顺序逐个发送。`}
      className="max-w-2xl"
      footer={
        <>
          {running ? (
            <Button variant="destructive" size="sm" onClick={abort}>
              <Square className="mr-1.5 h-3 w-3" />
              中止
            </Button>
          ) : (
            <Button variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
              关闭
            </Button>
          )}
          <Button size="sm" onClick={() => void run()} disabled={running || items.length === 0}>
            <Play className="mr-1.5 h-3 w-3" />
            {running ? '运行中…' : report ? '重新运行' : '开始运行'}
          </Button>
        </>
      }
    >
      {items.length === 0 ? (
        <p className="py-6 text-center text-xs text-muted-foreground">
          这个{kindLabel}下面还没有接口。
        </p>
      ) : (
        <>
          <div className="max-h-72 overflow-y-auto rounded-md border border-border">
            <table className="w-full text-left text-xs">
              <tbody>
                {items.map((item, index) => {
                  const outcome = outcomes[index] ?? null;
                  const isRunning = runningIndex === index;
                  return (
                    <tr key={item.endpointId} className="border-b border-border/60 last:border-0">
                      <td className="w-16 px-2 py-1.5 font-mono text-[10px] text-muted-foreground">
                        {item.method}
                      </td>
                      <td className="px-2 py-1.5">
                        <div className="truncate text-foreground">{item.name}</div>
                        <div className="truncate font-mono text-[10px] text-muted-foreground">
                          {item.url}
                        </div>
                      </td>
                      <td className="w-40 px-2 py-1.5 text-right">
                        <StatusCell outcome={outcome} isRunning={isRunning} />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          {report && <ReportPanel report={report} />}
        </>
      )}
    </Dialog>
  );
}

/** 一行的状态。 */
function StatusCell({ outcome, isRunning }: { outcome: BatchOutcome | null; isRunning: boolean }) {
  if (isRunning) {
    return <span className="text-muted-foreground">运行中…</span>;
  }
  if (!outcome) {
    return <span className="text-muted-foreground">待发</span>;
  }
  if (outcome.success) {
    return (
      <span className="text-muted-foreground">
        <span className="text-foreground">{outcome.status}</span>
        <span className="ml-2">{formatDuration(outcome.durationMs)}</span>
      </span>
    );
  }
  return (
    <span className="text-destructive" title={outcome.error}>
      {outcome.status > 0 ? outcome.status : failureLabel(outcome)}
      <span className="ml-2 text-muted-foreground">{formatDuration(outcome.durationMs)}</span>
    </span>
  );
}

/** 失败分类的简短说法。 */
function failureLabel(outcome: BatchOutcome): string {
  switch (outcome.failure) {
    case 'blocked':
      return '未发送';
    case 'aborted':
      return '已中止';
    case 'transport':
      return '无响应';
    default:
      return '失败';
  }
}

/** 报告面板。 */
function ReportPanel({ report }: { report: BatchReport }) {
  return (
    <div className="mt-3 rounded-md border border-border bg-muted/30 p-3 text-xs">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <Stat label="总数" value={report.total} />
        <Stat label="成功" value={report.success} tone="success" />
        <Stat label="失败" value={report.failed} tone={report.failed > 0 ? 'danger' : undefined} />
        {report.notRun > 0 && <Stat label="未执行" value={report.notRun} />}
        {report.blocked > 0 && <Stat label="未发送" value={report.blocked} />}
        {report.noResponse > 0 && <Stat label="无响应" value={report.noResponse} />}
        <Stat label="总耗时" value={formatDuration(report.totalDurationMs)} />
      </div>
      {report.statusCounts.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-muted-foreground">
          {report.statusCounts.map((s) => (
            <span key={s.status}>
              {s.status} × {s.count}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function Stat({
  label,
  value,
  tone,
}: {
  label: string;
  value: number | string;
  tone?: 'success' | 'danger';
}) {
  const toneClass =
    tone === 'success' ? 'text-foreground' : tone === 'danger' ? 'text-destructive' : '';
  return (
    <span>
      <span className="text-muted-foreground">{label}</span>{' '}
      <span className={`font-medium ${toneClass}`}>{value}</span>
    </span>
  );
}

/** 毫秒数写成人类读得懂的时长。 */
function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}
