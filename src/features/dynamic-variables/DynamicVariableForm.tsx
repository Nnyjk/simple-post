/**
 * 动态变量定义的表单 —— 新建与编辑共用一份。
 *
 * 数据源二选一：引用树里的一个接口（界面优先推荐这个形态——那个接口自己的 base URL、
 * 鉴权与前置脚本都会生效），或一份内联请求（method / url / headers / body / auth 的
 * JSON 文本，URL 必须写完整）。
 */
import { useMemo, useState } from 'react';
import { useAppStore } from '@/stores/app-store';
import type { DynamicVariableView, ValueMode } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';

export interface DynamicVariableFormProps {
  projectId: string;
  /** 传了就走编辑模式，不传就是新建。 */
  variable?: DynamicVariableView;
  onCancel: () => void;
  onDone: () => void;
  /** 编辑模式下的一键改名（面板的「改名建议」用）。 */
  onRename?: (name: string) => void;
}

export function DynamicVariableForm({
  projectId,
  variable,
  onCancel,
  onDone,
  onRename,
}: DynamicVariableFormProps) {
  const nodes = useAppStore((s) => s.nodes);
  const endpointDetails = useAppStore((s) => s.endpointDetails);
  const createDynamicVariable = useAppStore((s) => s.createDynamicVariable);
  const updateDynamicVariable = useAppStore((s) => s.updateDynamicVariable);

  const [name, setName] = useState(variable?.name ?? '');
  const [description, setDescription] = useState(variable?.description ?? '');
  // 数据源有两种形态，故形态本身是状态：它不能从「当前选中的端点」推出来——用户从
  // 「引用端点」切回「内联请求」时端点 id 还在，但它已经不生效了。
  const [sourceKind, setSourceKind] = useState<'endpoint' | 'inline'>(
    variable?.sourceEndpointId ? 'endpoint' : 'inline',
  );
  const [sourceEndpointId, setSourceEndpointId] = useState(variable?.sourceEndpointId ?? '');
  const [sourceInlineJson, setSourceInlineJson] = useState(variable?.sourceInlineJson ?? '');
  const [extractPath, setExtractPath] = useState(variable?.extractPath ?? '$');
  const [valueMode, setValueMode] = useState<ValueMode>(variable?.valueMode ?? 'auto');
  const [ttlSeconds, setTtlSeconds] = useState(String(variable?.ttlSeconds ?? 60));
  const [staleFallback, setStaleFallback] = useState(variable?.staleFallback ?? false);
  const [enabled, setEnabled] = useState(variable?.enabled ?? true);
  const [busy, setBusy] = useState(false);

  // 本项目的接口列表，供「引用端点」形态挑选。
  const endpoints = useMemo(
    () =>
      nodes
        .filter((n) => n.kind === 'endpoint' && n.rootId === projectId)
        .map((n) => ({ id: n.id, name: n.name, url: endpointDetails[n.id]?.url ?? '' })),
    [nodes, endpointDetails, projectId],
  );

  const usesEndpoint = sourceKind === 'endpoint';

  // 没有数据源的定义永远解析不出值，故界面不让人存：引用形态要选了接口，内联形态
  // 要填了请求。
  const hasSource = usesEndpoint ? sourceEndpointId !== '' : sourceInlineJson.trim() !== '';

  const handleSubmit = async () => {
    setBusy(true);
    try {
      if (variable) {
        await updateDynamicVariable(variable.id, {
          name,
          description: description || null,
          sourceEndpointId: usesEndpoint ? sourceEndpointId : null,
          sourceInlineJson: usesEndpoint ? null : sourceInlineJson || null,
          extractPath,
          valueMode,
          ttlSeconds: Number(ttlSeconds) || 60,
          staleFallback,
          enabled,
        });
      } else {
        await createDynamicVariable(projectId, {
          name,
          description: description || undefined,
          sourceEndpointId: usesEndpoint ? sourceEndpointId : null,
          sourceInlineJson: usesEndpoint ? undefined : sourceInlineJson || undefined,
          extractPath,
          valueMode,
          ttlSeconds: Number(ttlSeconds) || 60,
          staleFallback,
          enabled,
        });
      }
      onDone();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="rounded-md border border-border p-3">
      <h3 className="text-xs font-medium">{variable ? '编辑定义' : '新建定义'}</h3>
      <div className="mt-2 flex flex-col gap-2">
        <label className="flex items-center gap-2 text-xs">
          <span className="w-20 shrink-0 text-muted-foreground">变量名</span>
          <Input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="sns"
            className="font-mono"
          />
          <span className="text-[11px] text-muted-foreground">只能用字母、数字与下划线。</span>
        </label>

        <label className="flex items-center gap-2 text-xs">
          <span className="w-20 shrink-0 text-muted-foreground">说明</span>
          <Input
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="站点清单"
          />
        </label>

        <label className="flex items-start gap-2 text-xs">
          <span className="w-20 shrink-0 pt-1.5 text-muted-foreground">数据源</span>
          <div className="flex flex-1 flex-col gap-1">
            <select
              value={usesEndpoint ? sourceEndpointId : ''}
              onChange={(e) => {
                if (e.target.value === '') {
                  setSourceKind('inline');
                  setSourceEndpointId('');
                  return;
                }
                setSourceKind('endpoint');
                setSourceEndpointId(e.target.value);
              }}
              className="h-8 w-full rounded-md border border-input bg-background px-2 text-sm"
            >
              <option value="">内联请求</option>
              {endpoints.map((e) => (
                <option key={e.id} value={e.id}>
                  引用接口：{e.name}
                </option>
              ))}
            </select>
            {!usesEndpoint && (
              <textarea
                value={sourceInlineJson}
                onChange={(e) => setSourceInlineJson(e.target.value)}
                spellCheck={false}
                placeholder='{"method":"GET","url":"https://example.test/points","headers":[],"body":{"mode":"none","content":""},"auth":{"kind":"none"}}'
                className="min-h-20 w-full rounded-md border border-input bg-background px-2 py-1 font-mono text-xs"
              />
            )}
          </div>
        </label>

        <label className="flex items-center gap-2 text-xs">
          <span className="w-20 shrink-0 text-muted-foreground">抽取规则</span>
          <Input
            value={extractPath}
            onChange={(e) => setExtractPath(e.target.value)}
            placeholder="$"
            className="font-mono"
          />
          <span className="text-[11px] text-muted-foreground">JSONPath；`$` 表示整段响应。</span>
        </label>

        <label className="flex items-center gap-2 text-xs">
          <span className="w-20 shrink-0 text-muted-foreground">值的形态</span>
          <select
            value={valueMode}
            onChange={(e) => setValueMode(e.target.value as ValueMode)}
            className="h-8 w-40 rounded-md border border-input bg-background px-2 text-sm"
          >
            <option value="auto">auto</option>
            <option value="json">json</option>
            <option value="text">text</option>
          </select>
          <span className="text-[11px] text-muted-foreground">
            auto 与 text 输出相同；json 会给标量加引号。
          </span>
        </label>

        <label className="flex items-center gap-2 text-xs">
          <span className="w-20 shrink-0 text-muted-foreground">缓存时长</span>
          <Input
            type="number"
            min={0}
            value={ttlSeconds}
            onChange={(e) => setTtlSeconds(e.target.value)}
            className="w-24"
          />
          <span className="text-[11px] text-muted-foreground">秒。</span>
        </label>

        <label className="flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={staleFallback}
            onChange={(e) => setStaleFallback(e.target.checked)}
          />
          <span>取数失败时允许用过期值兜底</span>
        </label>

        <label className="flex items-center gap-2 text-xs">
          <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
          <span>启用（停用后不参与替换）</span>
        </label>

        {variable?.sourceEndpointName && (
          <p className="text-[11px] text-muted-foreground">
            当前数据源：{variable.sourceEndpointName}
          </p>
        )}

        <div className="mt-1 flex items-center gap-2">
          <Button
            size="sm"
            onClick={() => void handleSubmit()}
            disabled={busy || name.trim() === '' || !hasSource}
          >
            保存
          </Button>
          <Button variant="ghost" size="sm" onClick={onCancel}>
            取消
          </Button>
          {variable && onRename && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                onRename(name);
                onDone();
              }}
            >
              只改名
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
