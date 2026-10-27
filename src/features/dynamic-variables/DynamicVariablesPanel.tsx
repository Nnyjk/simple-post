/**
 * 动态变量面板 —— 项目视图里的独立页面。
 *
 * 一条动态变量是一份声明：变量名 → 数据源 → 抽取规则 → 缓存时长。消费端点的请求体里
 * 写 `{{sns}}`，发送时程序取数、抽取、缓存并替换。这个面板负责声明它、显示它当前的
 * 值、并在取数失败时把原因说清楚。
 *
 * 面板读的是 store 里的 `dynamicVariables`（列表命令一次带回定义 + 当前值 + 缓存状态），
 * 故它不自己发请求；取数与写覆盖值都走 store 动作。
 */
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useAppStore } from '@/stores/app-store';
import type { DynamicVariableView, ValueMode } from '@/types/domain';
import { listDynamicNames } from '@/lib/dynamic-vars';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { DynamicVariableForm } from './DynamicVariableForm';

/** 面板的属性。标签 id 就是项目 id，故项目由标签带来。 */
export interface DynamicVariablesPanelProps {
  projectId: string;
}

export function DynamicVariablesPanel({ projectId }: DynamicVariablesPanelProps) {
  const variables = useAppStore((s) => s.dynamicVariables);
  const resolving = useAppStore((s) => s.dynamicResolving);
  const error = useAppStore((s) => s.dynamicError);
  const loadDynamicVariables = useAppStore((s) => s.loadDynamicVariables);
  const resolveDynamicVariables = useAppStore((s) => s.resolveDynamicVariables);
  const setDynamicOverride = useAppStore((s) => s.setDynamicOverride);
  const refreshDynamicVariable = useAppStore((s) => s.refreshDynamicVariable);
  const deleteDynamicVariable = useAppStore((s) => s.deleteDynamicVariable);
  const updateDynamicVariable = useAppStore((s) => s.updateDynamicVariable);
  const endpointDetails = useAppStore((s) => s.endpointDetails);
  const nodes = useAppStore((s) => s.nodes);
  const environments = useAppStore((s) => s.environments);
  const activeEnvironmentId = useAppStore((s) => s.activeEnvironmentId);

  const [editingId, setEditingId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    void loadDynamicVariables(projectId, activeEnvironmentId || undefined);
  }, [projectId, activeEnvironmentId, loadDynamicVariables]);

  // 哪些变量名出现在本项目的请求里。界面上没有每个端点解析出的 base URL，故这里
  // 只扫端点自己的字段；base URL 里的标记极少，且那是环境的内容。
  const referencedBy = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const node of nodes) {
      if (node.kind !== 'endpoint') continue;
      const detail = endpointDetails[node.id];
      if (!detail) continue;
      const names = listDynamicNames({
        baseUrl: '',
        url: detail.url,
        params: detail.params,
        pathParams: detail.pathParams,
        headers: detail.headers,
        body: detail.body,
        auth: detail.auth,
      });
      for (const name of names) {
        const list = map.get(name) ?? [];
        list.push(node.name);
        map.set(name, list);
      }
    }
    return map;
  }, [nodes, endpointDetails]);

  // 请求里出现、但本项目没有定义的变量名。给出一个最接近的动态变量名，供一键改名。
  const suggestions = useMemo(() => {
    const defined = new Set(variables.map((v) => v.name));
    const envNames = new Set<string>();
    for (const env of environments) {
      for (const key of Object.keys(env.variables)) envNames.add(key);
    }
    const out: { used: string; suggestion: string | null }[] = [];
    for (const name of referencedBy.keys()) {
      if (defined.has(name) || envNames.has(name)) continue;
      let best: string | null = null;
      let bestDistance = Number.POSITIVE_INFINITY;
      for (const candidate of defined) {
        const d = editDistance(name, candidate);
        if (d < bestDistance) {
          bestDistance = d;
          best = candidate;
        }
      }
      // 差得太多就不提了——那不是「写错了一个字母」，是另一个东西。
      const limit = Math.max(2, Math.floor(name.length / 2));
      out.push({ used: name, suggestion: best !== null && bestDistance <= limit ? best : null });
    }
    return out;
  }, [referencedBy, variables, environments]);

  const handleResolveAll = useCallback(async () => {
    const names = variables.map((v) => v.name);
    await resolveDynamicVariables(projectId, names, activeEnvironmentId || undefined);
    // 解析的结果落在 store 的取值桶里，而卡片读的是列表——故这里要重载一次列表，
    // 否则值、来源徽标与取数时间都不动，看着像按钮坏了。
    await loadDynamicVariables(projectId, activeEnvironmentId || undefined);
  }, [variables, projectId, activeEnvironmentId, resolveDynamicVariables, loadDynamicVariables]);

  const handleRefresh = useCallback(
    async (variable: DynamicVariableView) => {
      await refreshDynamicVariable(projectId, variable.id, activeEnvironmentId || undefined);
    },
    [projectId, activeEnvironmentId, refreshDynamicVariable],
  );

  const handleClearOverride = useCallback(
    async (variable: DynamicVariableView) => {
      await setDynamicOverride(variable.id, null, {
        environmentId: activeEnvironmentId || undefined,
      });
    },
    [activeEnvironmentId, setDynamicOverride],
  );

  const handleRename = useCallback(
    async (variable: DynamicVariableView, name: string) => {
      await updateDynamicVariable(variable.id, { name });
    },
    [updateDynamicVariable],
  );

  return (
    <div className="h-full min-h-0 overflow-y-auto px-4 pb-8">
      <div className="mx-auto flex max-w-3xl flex-col gap-4 pt-4">
        <header className="flex items-center gap-2">
          <h2 className="text-sm font-medium">动态变量</h2>
          <span className="text-xs text-muted-foreground">
            在请求里写 {'{{变量名}}'}，发送时自动取数并替换。
          </span>
          <div className="ml-auto flex items-center gap-2">
            <Button variant="outline" size="sm" onClick={() => void handleResolveAll()}>
              全部刷新
            </Button>
            <Button size="sm" onClick={() => setCreating(true)}>
              新建定义
            </Button>
          </div>
        </header>

        {error && (
          <p className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            {error}
          </p>
        )}

        {variables.length === 0 && !creating && (
          <p className="rounded-md border border-dashed border-border px-3 py-6 text-center text-xs text-muted-foreground">
            本项目还没有动态变量。新建一条：给它一个名字、一个数据源（引用树里的接口，或一份内联请求）、
            一条 JSONPath 抽取规则与一个缓存时长。
          </p>
        )}

        {variables.map((variable) => (
          <DynamicVariableCard
            key={variable.id}
            variable={variable}
            resolving={resolving}
            referencedBy={referencedBy.get(variable.name) ?? []}
            editing={editingId === variable.id}
            onEdit={() => setEditingId(variable.id)}
            onCancelEdit={() => setEditingId(null)}
            onRefresh={() => void handleRefresh(variable)}
            onClearOverride={() => void handleClearOverride(variable)}
            onDelete={() => void deleteDynamicVariable(variable.id)}
            onRename={(name) => void handleRename(variable, name)}
          />
        ))}

        {creating && (
          <DynamicVariableForm
            projectId={projectId}
            onCancel={() => setCreating(false)}
            onDone={() => setCreating(false)}
          />
        )}

        {suggestions.length > 0 && (
          <section className="rounded-md border border-border p-3">
            <h3 className="text-xs font-medium">请求里出现、但本项目没有定义的名字</h3>
            <ul className="mt-2 flex flex-col gap-1">
              {suggestions.map((item) => {
                // 建议的名字取自本项目的定义集合，故必然找得到；找不到就不给建议。
                const target = item.suggestion
                  ? variables.find((v) => v.name === item.suggestion)
                  : undefined;
                return (
                  <li key={item.used} className="flex items-center gap-2 text-xs">
                    <code className="font-mono text-muted-foreground">{`{{${item.used}}}`}</code>
                    {target ? (
                      <>
                        <span className="text-muted-foreground">本项目有个叫</span>
                        <code className="font-mono">{target.name}</code>
                        <RenameButton
                          variable={target}
                          onRename={(name) => void handleRename(target, name)}
                        />
                      </>
                    ) : (
                      <span className="text-muted-foreground">本项目没有相近的动态变量。</span>
                    )}
                  </li>
                );
              })}
            </ul>
          </section>
        )}
      </div>
    </div>
  );
}

/** 一条定义 + 它的当前值与缓存状态。 */
interface DynamicVariableCardProps {
  variable: DynamicVariableView;
  resolving: boolean;
  referencedBy: string[];
  editing: boolean;
  onEdit: () => void;
  onCancelEdit: () => void;
  onRefresh: () => void;
  onClearOverride: () => void;
  onDelete: () => void;
  /** 一键改名：表单里的「只改名」按钮用它。 */
  onRename: (name: string) => void;
}

function DynamicVariableCard({
  variable,
  resolving,
  referencedBy,
  editing,
  onEdit,
  onCancelEdit,
  onRefresh,
  onClearOverride,
  onDelete,
  onRename,
}: DynamicVariableCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [draft, setDraft] = useState(variable.value ?? '');
  const setDynamicOverride = useAppStore((s) => s.setDynamicOverride);
  const activeEnvironmentId = useAppStore((s) => s.activeEnvironmentId);
  // 取数失败的原因。列表视图本身不带它（那是一次纯读），故取数时记进 store 的那份。
  const dynamicError = useAppStore((s) => s.dynamicErrors[variable.name]);
  const error = variable.error ?? dynamicError;

  /** 打开编辑器时把草稿重置成当前值——否则上一次没保存的编辑会留在框里。 */
  const openEditor = () => {
    setDraft(variable.value ?? '');
    setExpanded(true);
  };

  // 数据源。两个来源字段都为空只有一个成因：引用的接口被删了（外键把它置成 NULL）——
  // 界面不允许建一个没有数据源的定义，故「没有数据源」与「数据源已删除」是同一件事。
  const sourceMissing = !variable.sourceEndpointId && !variable.sourceInlineJson;
  const source = variable.sourceEndpointId
    ? variable.sourceEndpointName
      ? `引用接口：${variable.sourceEndpointName}`
      : '引用接口（已被删除）'
    : variable.sourceInlineJson
      ? '内联请求'
      : '数据源已删除';

  return (
    <article className="rounded-md border border-border p-3">
      <div className="flex items-center gap-2">
        <span className="font-mono text-sm">{variable.name}</span>
        <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px] text-muted-foreground">
          {`{{${variable.name}}}`}
        </code>
        <StatusBadge source={variable.source} />
        <span className="ml-auto flex items-center gap-1">
          <Button
            variant="ghost"
            size="xs"
            onClick={onRefresh}
            disabled={resolving}
            title={variable.isOverride ? '重新取数会放弃手动指定的值' : '跳过缓存，重新取一次数'}
          >
            刷新
          </Button>
          {variable.isOverride && (
            <Button variant="ghost" size="xs" onClick={onClearOverride}>
              恢复自动获取
            </Button>
          )}
          <Button variant="ghost" size="xs" onClick={onEdit}>
            编辑
          </Button>
          <Button variant="ghost" size="xs" onClick={onDelete}>
            删除
          </Button>
        </span>
      </div>

      <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
        <dt className="text-muted-foreground">数据源</dt>
        <dd className={sourceMissing ? 'text-destructive' : undefined}>{source}</dd>
        <dt className="text-muted-foreground">抽取规则</dt>
        <dd className="font-mono">{variable.extractPath}</dd>
        <dt className="text-muted-foreground">缓存时长</dt>
        <dd>{variable.ttlSeconds} 秒</dd>
        {referencedBy.length > 0 && (
          <>
            <dt className="text-muted-foreground">被引用</dt>
            <dd>{referencedBy.join('、')}</dd>
          </>
        )}
      </dl>

      <div className="mt-2">
        <div className="flex items-center gap-2 text-xs">
          <span className="text-muted-foreground">当前值</span>
          <button
            type="button"
            className="truncate font-mono text-muted-foreground hover:text-foreground"
            onClick={() => setExpanded((v) => !v)}
          >
            {variable.value ? summarize(variable.value) : '（没有值）'}
          </button>
          <Button variant="ghost" size="xs" onClick={openEditor}>
            {expanded ? '收起' : '编辑'}
          </Button>
        </div>
        {expanded && (
          <div className="mt-2 flex flex-col gap-2">
            <textarea
              className="min-h-24 w-full rounded-md border border-input bg-background px-2 py-1 font-mono text-xs"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              spellCheck={false}
            />
            <div className="flex items-center gap-2">
              <Button
                size="xs"
                onClick={() =>
                  void setDynamicOverride(variable.id, draft, {
                    environmentId: activeEnvironmentId || undefined,
                  })
                }
              >
                保存为覆盖值
              </Button>
              <span className="text-[11px] text-muted-foreground">
                覆盖值优先于自动取回的值；过期时间默认沿用 TTL。
              </span>
            </div>
          </div>
        )}
      </div>

      <p className="mt-2 text-[11px] text-muted-foreground">
        {variable.fetchedAt ? `取数于 ${formatTime(variable.fetchedAt)}` : '尚未取数'}
        {variable.expiresAt ? ` · 过期于 ${formatTime(variable.expiresAt)}` : ''}
      </p>
      {error && (
        <p className="mt-1 text-[11px] text-destructive">{`${error.kind}：${error.message}`}</p>
      )}

      {editing && (
        <DynamicVariableForm
          projectId={variable.projectId}
          variable={variable}
          onCancel={onCancelEdit}
          onDone={onCancelEdit}
          onRename={onRename}
        />
      )}
    </article>
  );
}

/** 一键把引用它的请求改成正确的名字。 */
function RenameButton({
  variable,
  onRename,
}: {
  variable: DynamicVariableView | undefined;
  onRename: (name: string) => void;
}) {
  const [name, setName] = useState(variable?.name ?? '');
  if (!variable) return null;
  return (
    <span className="flex items-center gap-1">
      <Input
        value={name}
        onChange={(e) => setName(e.target.value)}
        className="h-6 w-28 font-mono text-xs"
      />
      <Button variant="ghost" size="xs" onClick={() => onRename(name)}>
        改名
      </Button>
    </span>
  );
}

/** 当前值的来源。 */
function StatusBadge({ source }: { source: DynamicVariableView['source'] }) {
  const label =
    source === 'override'
      ? '手动改的'
      : source === 'stale'
        ? '已过期'
        : source === 'auto'
          ? '自动取回'
          : '没有值';
  return (
    <span className="rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground">
      {label}
    </span>
  );
}

/** 值的摘要：长度加开头。 */
function summarize(value: string): string {
  const head = value.length > 80 ? `${value.slice(0, 80)}…` : value;
  return `${head}（${value.length} 字符）`;
}

/** 时间戳 → 本地时刻。 */
function formatTime(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/** 两个名字的编辑距离。用来判断「用户是不是把名字写错了一个字母」。 */
function editDistance(a: string, b: string): number {
  const rows = a.length + 1;
  const cols = b.length + 1;
  const table: number[] = new Array(rows * cols).fill(0);
  for (let i = 0; i < rows; i++) table[i * cols] = i;
  for (let j = 0; j < cols; j++) table[j] = j;
  for (let i = 1; i < rows; i++) {
    for (let j = 1; j < cols; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      table[i * cols + j] = Math.min(
        table[(i - 1) * cols + j] + 1,
        table[i * cols + (j - 1)] + 1,
        table[(i - 1) * cols + (j - 1)] + cost,
      );
    }
  }
  return table[(rows - 1) * cols + (cols - 1)];
}

export type { ValueMode };
