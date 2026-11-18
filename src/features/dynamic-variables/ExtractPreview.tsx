/**
 * 「预览取数」—— 用表单里**当前这份草稿**真发一次取数请求，把响应的 JSON 摊成一棵可点的树。
 *
 * 存在的理由：抽取规则是一条要手写的 JSONPath，而用户往往并不知道接口到底返回什么形状，
 * 而「存下来、去请求里发一次、再回来看值」这条回路太长。预览让「点哪个节点就拿哪条路径」
 * 一步到位，并且**在把路径写进抽取规则之前**就把抽取结果与最终值给出来。
 *
 * 三条硬约束（后端 `dynamic::preview` 是同一套语义，界面只是照着说）：
 *
 * 1. **不碰持久状态**：不读不写缓存、不写覆盖值、不写响应历史，故重复点预览是幂等的，
 *    手写的覆盖值不会被冲掉。
 * 2. **先取数、后抽取**：抽取或处理脚本失败不让预览失败——正文照给，树照样能点。
 *    这正是用户把一条抽不到的路径改对所需要的东西。
 * 3. **环境与真实取数用的是同一个**：草稿点名了取数环境就发它，没点名（继承）就发调用方
 *    此刻选中的环境——与 `dynamicVariablesResolve` 送的是同一个值（store 的
 *    `callerEnvironmentId` 也只回答「用户此刻选的是哪个」）。
 *
 * 预览结果**不进 store**：它是「这份草稿这一刻」的一次性产物，进全局状态就会在换定义、
 * 换环境之后仍被当成当前值。故它只活在这个组件里，并且每条结果都标清自己对应哪份草稿。
 */
import { useState } from 'react';
import { useAppStore } from '@/stores/app-store';
import type { DynamicPreviewResult } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { errorText } from '@/lib/error-text';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { buildScriptSnippet, shouldConfirmOverwrite } from './extract-path';
import { PreviewJsonTree } from './PreviewJsonTree';

export interface ExtractPreviewProps {
  /** 数据源是「引用接口」还是「内联请求」。内联请求没有环境那一格。 */
  usesEndpoint: boolean;
  sourceEndpointId: string;
  /** 引用接口时它已经选中的那个接口的名字（结果面板用它标清「这份结果对应哪份草稿」）。 */
  sourceEndpointName: string;
  sourceInlineJson: string;
  /** 表单里「取数环境」那一格。空串 = **继承当前环境**。 */
  environmentId: string;
  /** 抽取规则原文。 */
  extractPath: string;
  /** 处理脚本原文。 */
  transformScript: string;
  /** 点了树上的节点：把它的 JSONPath 写进抽取规则。 */
  onPickPath: (path: string) => void;
  /** 「生成脚本片段」：把片段写进处理脚本。 */
  onUseScript: (code: string) => void;
}

/**
 * 这份结果对应哪份草稿 —— 取数那一刻的快照。
 *
 * 结果面板必然与表单分处两个时刻：用户点完预览就会照着树改抽取规则。若不标清，一份旧
 * 响应配着新数据源，用户会以为「抽出来的是这个新接口的东西」。
 */
interface DraftSnapshot {
  /** 数据源的一句话描述（引用接口时带名字或 id）。 */
  source: string;
  /** 取数时的抽取规则原文（空串按「整段响应」显示成 `$`）。 */
  extractPath: string;
  /** 取数时有没有处理脚本。 */
  hasScript: boolean;
  /** 结果里实际生效的环境名。`null` = 没有环境。 */
  environment: string | null;
}

/** 一行摘要把值压短：预览是给人瞄一眼的，不是导出器。 */
function summarizeValue(value: unknown): string {
  const text = JSON.stringify(value) ?? 'undefined';
  return text.length > 160 ? `${text.slice(0, 160)}…` : text;
}

export function ExtractPreview({
  usesEndpoint,
  sourceEndpointId,
  sourceEndpointName,
  sourceInlineJson,
  environmentId,
  extractPath,
  transformScript,
  onPickPath,
  onUseScript,
}: ExtractPreviewProps) {
  const environments = useAppStore((s) => s.environments);
  // 调用方此刻选中的环境：草稿「继承取数环境」时，后端就用它去取数。与解析走同一个值。
  const activeEnvironmentId = useAppStore((s) => s.activeEnvironmentId);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<DynamicPreviewResult | null>(null);
  const [snapshot, setSnapshot] = useState<DraftSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** 当前选中的节点路径（树上点的那一条）。它就是「生成脚本片段」的起点。 */
  const [pickedPath, setPickedPath] = useState<string | null>(null);
  /** 待写入脚本的片段。有它就说明「覆写确认」还在等人点头。 */
  const [pendingScript, setPendingScript] = useState<string | null>(null);
  /** 刚写进脚本那一段的说明，告诉用户这条片段以什么为起点。 */
  const [snippetHint, setSnippetHint] = useState<string | null>(null);

  const hasSource = usesEndpoint ? sourceEndpointId !== '' : sourceInlineJson.trim() !== '';

  // 表单当前这份草稿的指纹：与快照不同就说明结果已经不对应眼前这份草稿了。
  const draftChanged =
    snapshot !== null &&
    (snapshot.extractPath !== (extractPath.trim() === '' ? '$' : extractPath.trim()) ||
      snapshot.hasScript !== (transformScript.trim() !== ''));

  const runPreview = async () => {
    setBusy(true);
    setError(null);
    setSnippetHint(null);
    try {
      // 字段集合与 `handleSubmit` 的保存分支一致：数据源二选一（另一种显式清空）、点名的
      // 取数环境（内联请求没有这一格）、抽取规则与脚本。
      //
      // `environmentId` 是**调用方此刻选中的环境**，不是上面那一格：草稿没点名环境时，
      // 后端就是拿它去取数的（与 `dynamicVariablesResolve` 送同一个值）。
      const preview = await invokeCmd<DynamicPreviewResult>(COMMANDS.dynamicVariablesPreview, {
        sourceEndpointId: usesEndpoint ? sourceEndpointId || null : null,
        sourceInlineJson: usesEndpoint ? null : sourceInlineJson || null,
        sourceEnvironmentId: usesEndpoint ? environmentId || null : null,
        extractPath,
        transformScript,
        environmentId: activeEnvironmentId || null,
      });
      const environment =
        environments.find((e) => e.id === (preview.environmentId ?? ''))?.name ??
        preview.environmentId ??
        null;
      setSnapshot({
        source: usesEndpoint
          ? `引用接口 ${sourceEndpointName || sourceEndpointId || '（未选）'}`
          : '内联请求',
        extractPath: preview.extractPath,
        hasScript: transformScript.trim() !== '',
        environment,
      });
      setResult(preview);
      setPickedPath(null);
    } catch (e) {
      setResult(null);
      setSnapshot(null);
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const pick = (path: string) => {
    setPickedPath(path);
    onPickPath(path);
    setSnippetHint(null);
  };

  const generateSnippet = () => {
    if (!pickedPath) return;
    const snippet = buildScriptSnippet(pickedPath, extractPath);
    if (shouldConfirmOverwrite(transformScript)) {
      // 用户可能手写过脚本：便利不能替人做主，先问一句。
      setPendingScript(snippet.code);
      setSnippetHint(snippet.hint);
      return;
    }
    onUseScript(snippet.code);
    setSnippetHint(snippet.hint);
  };

  const applyPendingScript = () => {
    if (pendingScript === null) return;
    onUseScript(pendingScript);
    setPendingScript(null);
  };

  return (
    <div className="flex flex-col gap-1.5 rounded-md border border-border bg-muted/20 p-2">
      <div className="flex items-center gap-2 text-xs">
        <Button
          size="sm"
          variant="ghost"
          onClick={() => void runPreview()}
          disabled={busy || !hasSource}
        >
          {busy ? '取数中…' : '预览取数'}
        </Button>
        <span className="text-micro text-muted-foreground">
          用上面这份草稿真发一次请求，点响应里的节点就把它的 JSONPath 写进抽取规则。不会碰缓存与
          覆盖值。
        </span>
      </div>
      {!hasSource && (
        <p className="text-micro text-muted-foreground">先选好数据源（接口或内联请求）再预览。</p>
      )}
      {error && (
        <p className="rounded-md border border-destructive/40 bg-destructive/10 px-2 py-1 text-micro text-destructive">
          取数失败：{error}
        </p>
      )}
      {result && snapshot && (
        <div className="flex flex-col gap-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-micro text-muted-foreground">
            <span className="font-mono text-foreground">{result.status}</span>
            <span>{result.durationMs} ms</span>
            <span className="font-mono">{result.contentType || '（无）'}</span>
            <span>
              环境：
              {snapshot.environment === null ? '没有环境' : snapshot.environment}
            </span>
            <span className="text-faint">
              ← 这份结果对应{snapshot.source}，抽取规则{' '}
              <code className="font-mono">{snapshot.extractPath}</code>，
              {snapshot.hasScript ? '带处理脚本' : '没有处理脚本'}
            </span>
          </div>
          {draftChanged && (
            <p className="text-micro text-warning">
              草稿已经改过了：下面这份响应是上一次预览的，改完点「预览取数」再重取一次。
            </p>
          )}
          {result.truncated && (
            <p className="text-micro text-warning">
              正文被上限截断了：树只覆盖前面那一截，后面还有内容没显示。
            </p>
          )}
          {result.binary && (
            <p className="text-micro text-warning">
              这是二进制正文（图片、压缩包等），不能做 JSONPath，故没有可点的树。
            </p>
          )}
          {result.extractError && (
            <p className="rounded-md border border-destructive/40 bg-destructive/10 px-2 py-1 text-micro text-destructive">
              抽取失败（{result.extractError.kind}）：{result.extractError.message}
              。正文照给，照着树重新点一条路径即可。
            </p>
          )}
          {result.transformError && (
            <p className="rounded-md border border-destructive/40 bg-destructive/10 px-2 py-1 text-micro text-destructive">
              处理脚本失败：{result.transformError.message}。最终值仍是抽取结果。
            </p>
          )}
          {result.extracted !== undefined && (
            <p className="text-micro text-muted-foreground">
              抽取结果 <code className="font-mono">{summarizeValue(result.extracted)}</code>
              {'；最终值 '}
              <code className="font-mono">
                {result.value === undefined ? '（无）' : summarizeValue(result.value)}
              </code>
            </p>
          )}
          {result.bodyJson !== undefined ? (
            <div className="max-h-64 overflow-auto rounded border border-border bg-background/60 p-1.5">
              <PreviewJsonTree value={result.bodyJson} path="$" onPick={pick} />
            </div>
          ) : (
            !result.binary && (
              <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded border border-border bg-background/60 p-1.5 font-mono text-micro text-muted-foreground">
                {result.bodyText === '' ? '（正文为空）' : result.bodyText}
              </pre>
            )
          )}
          {pickedPath && (
            <div className="flex flex-wrap items-center gap-2 text-micro">
              <span className="text-muted-foreground">已写入抽取规则：</span>
              <code className="font-mono text-foreground">{pickedPath}</code>
              <Button size="sm" variant="ghost" onClick={generateSnippet}>
                生成脚本片段
              </Button>
              {snippetHint && <span className="text-faint">{snippetHint}</span>}
            </div>
          )}
        </div>
      )}
      <ConfirmDialog
        open={pendingScript !== null}
        onOpenChange={(open) => {
          if (!open) setPendingScript(null);
        }}
        title="覆盖现有的处理脚本？"
        description="处理脚本里已经有内容。生成片段会把它整段换掉，手写的那部分就没了。"
        confirmText="覆盖"
        onConfirm={applyPendingScript}
      />
    </div>
  );
}
