/**
 * 动态变量定义的表单 —— 新建与编辑共用一份。
 *
 * 数据源二选一：引用树里的一个接口（界面优先推荐这个形态——那个接口自己的 base URL、
 * 鉴权与前置脚本都会生效），或一份内联请求（method / url / headers / body / auth 的
 * JSON 文本，URL 必须写完整）。
 *
 * 引用接口时，表单还会给出**取数用的环境**：默认取那个接口所属项目的第一个可用
 * 环境，用户可以改。它跟着数据源走，故选了接口之后它才有意义。取数用的地址由那个
 * 接口所属模块在所选环境里的那一格决定，故表单不必再问一次。
 *
 * 定义挂在**项目**里，故表单要知道它属于哪个项目：新建时是当前项目，编辑时是这条定义
 * 自己的归属项目。变量名在项目内唯一。
 */
import { useMemo, useState } from 'react';
import { useAppStore } from '@/stores/app-store';
import type { DynamicVariableView, ValueMode } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { EndpointTreePicker } from './EndpointTreePicker';

export interface DynamicVariableFormProps {
  /** 传了就走编辑模式，不传就是新建。 */
  variable?: DynamicVariableView;
  /**
   * 归属项目。新建时表单把它写进 `projectId`；编辑时表单优先用 `variable.projectId`，
   * 那个值缺失时用这个 prop，两个都缺时用当前项目。
   */
  projectId?: string;
  onCancel: () => void;
  onDone: () => void;
  /** 编辑模式下的一键改名（面板的「改名建议」用）。 */
  onRename?: (name: string) => void;
}

/** 按代码点比两个字符串。SQLite 的 `ORDER BY name` 比的是 UTF-8 字节，JS 的 `<` 比的是
 * UTF-16 码元 —— 对 BMP 里的常用字符两者一致，故这里直接用 `<`。 */
function cmp(a: string, b: string): number {
  if (a === b) return 0;
  return a < b ? -1 : 1;
}

export function DynamicVariableForm({
  variable,
  projectId,
  onCancel,
  onDone,
  onRename,
}: DynamicVariableFormProps) {
  const projects = useAppStore((s) => s.projects);
  const environments = useAppStore((s) => s.environments);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const createDynamicVariable = useAppStore((s) => s.createDynamicVariable);
  const updateDynamicVariable = useAppStore((s) => s.updateDynamicVariable);
  // 保存失败的原因。表单就地显示它，免得用户只看到表单凭空消失。
  const saveError = useAppStore((s) => s.dynamicError);

  // 归属项目。编辑时以定义自己的为准，新建时是当前项目。
  const ownerProjectId = variable?.projectId ?? projectId ?? activeProjectId;

  const [name, setName] = useState(variable?.name ?? '');
  const [description, setDescription] = useState(variable?.description ?? '');
  // 数据源有两种形态，故形态本身是状态：它不能从「当前选中的端点」推出来——用户从
  // 「引用端点」切回「内联请求」时端点 id 还在，但它已经不生效了。
  //
  // 「数据源接口已被删除」的定义落在**引用接口**形态：那条路是唯一能救它的（重新挑一个
  // 接口），而内联形态会让用户以为要手写一份请求。
  const sourceDeleted = !!variable && !variable.sourceEndpointId && !variable.sourceInlineJson;
  const [sourceKind, setSourceKind] = useState<'endpoint' | 'inline'>(
    variable ? (variable.sourceEndpointId || sourceDeleted ? 'endpoint' : 'inline') : 'inline',
  );
  const [sourceEndpointId, setSourceEndpointId] = useState(variable?.sourceEndpointId ?? '');
  const [sourceProjectId, setSourceProjectId] = useState(variable?.sourceProjectId ?? '');
  // 数据源接口的名字。树上点一下就有，不必再查一次树。
  const [sourceEndpointName, setSourceEndpointName] = useState(variable?.sourceEndpointName ?? '');
  const [sourceInlineJson, setSourceInlineJson] = useState(variable?.sourceInlineJson ?? '');
  // 取数用的环境。空串 = 跟随默认（数据源接口所属项目的首个可用环境）。
  const [environmentId, setEnvironmentId] = useState(variable?.sourceEnvironmentId ?? '');
  const [extractPath, setExtractPath] = useState(variable?.extractPath ?? '$');
  const [transformScript, setTransformScript] = useState(variable?.transformScript ?? '');
  const [valueMode, setValueMode] = useState<ValueMode>(variable?.valueMode ?? 'auto');
  const [ttlSeconds, setTtlSeconds] = useState(String(variable?.ttlSeconds ?? 60));
  const [staleFallback, setStaleFallback] = useState(variable?.staleFallback ?? false);
  const [enabled, setEnabled] = useState(variable?.enabled ?? true);
  const [busy, setBusy] = useState(false);

  const usesEndpoint = sourceKind === 'endpoint';

  // 项目里的环境，按名字排序 —— 「第一个可用环境」就是列表里的头一个。
  //
  // 后端 `core::dynamic::source_context` 选默认环境时用的是 `list_by_project` 的
  // `ORDER BY name ASC, id ASC`，故这里必须按同一套顺序排：界面上写的「默认（X）」
  // 与真正生效的那个是同一个，否则标签与行为各说各话。
  const projectEnvironments = useMemo(
    () =>
      environments
        .filter((e) => e.projectId === sourceProjectId)
        .sort((a, b) => (a.name === b.name ? cmp(a.id, b.id) : cmp(a.name, b.name))),
    [environments, sourceProjectId],
  );
  const firstEnvironment = projectEnvironments[0];

  const handleSelectEndpoint = (endpointId: string, projectId: string, endpointName: string) => {
    setSourceKind('endpoint');
    setSourceEndpointId(endpointId);
    setSourceProjectId(projectId);
    setSourceEndpointName(endpointName);
    setEnvironmentId('');
  };

  // 没有数据源的定义永远解析不出值，故界面不让人存：引用形态要选了接口，内联形态
  // 要填了请求。
  const hasSource = usesEndpoint ? sourceEndpointId !== '' : sourceInlineJson.trim() !== '';

  const handleSubmit = async () => {
    setBusy(true);
    try {
      // 缓存时长：0 是有意义的（不缓存，每次重新取数），故不能用 `|| 60`——那会把 0
      // 变成假值再回退成 60。
      const ttl = Number(ttlSeconds);
      const ttlValue = Number.isFinite(ttl) ? ttl : 60;
      if (variable) {
        const ok = await updateDynamicVariable(variable.id, {
          name,
          description: description || null,
          sourceEndpointId: usesEndpoint ? sourceEndpointId : null,
          sourceInlineJson: usesEndpoint ? null : sourceInlineJson || null,
          sourceEnvironmentId: usesEndpoint ? environmentId || null : null,
          extractPath,
          transformScript: transformScript.trim() === '' ? null : transformScript,
          valueMode,
          ttlSeconds: ttlValue,
          staleFallback,
          enabled,
        });
        // 保存失败时表单留着：用户填的内容不丢。
        if (!ok) return;
      } else {
        const created = await createDynamicVariable({
          projectId: ownerProjectId,
          name,
          description: description || undefined,
          sourceEndpointId: usesEndpoint ? sourceEndpointId : null,
          sourceInlineJson: usesEndpoint ? null : sourceInlineJson || null,
          sourceEnvironmentId: usesEndpoint ? environmentId || null : null,
          extractPath,
          transformScript: transformScript.trim() === '' ? null : transformScript,
          valueMode,
          ttlSeconds: ttlValue,
          staleFallback,
          enabled,
        });
        if (!created) return;
      }
      onDone();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="rounded-md border border-border p-3">
      <h3 className="text-xs font-medium">{variable ? '编辑定义' : '新建定义'}</h3>
      <p className="mt-1 text-[11px] text-muted-foreground">
        归属项目：{projects.find((p) => p.id === ownerProjectId)?.name ?? '（未知）'}
        。变量名在这个项目里唯一。
      </p>
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

        <div className="flex items-start gap-2 text-xs">
          <span className="w-20 shrink-0 pt-1.5 text-muted-foreground">数据源</span>
          <div className="flex flex-1 flex-col gap-1">
            <div className="flex items-center gap-2">
              <select
                value={sourceKind}
                onChange={(e) => setSourceKind(e.target.value as 'endpoint' | 'inline')}
                className="h-8 rounded-md border border-input bg-background px-2 text-sm"
              >
                <option value="endpoint">引用接口</option>
                <option value="inline">内联请求</option>
              </select>
              {usesEndpoint && sourceEndpointId && (
                <span className="truncate text-[11px] text-muted-foreground">
                  已选接口 {sourceEndpointName || sourceEndpointId}
                </span>
              )}
            </div>
            {sourceDeleted && (
              <p className="text-[11px] text-destructive">
                这条定义引用的接口已被删除，故它现在没有数据源。挑一个接口替上它即可救回这条定义。
              </p>
            )}
            {usesEndpoint && (
              <EndpointTreePicker value={sourceEndpointId} onSelect={handleSelectEndpoint} />
            )}
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
        </div>

        {usesEndpoint && sourceEndpointId !== '' && (
          <>
            <label className="flex items-center gap-2 text-xs">
              <span className="w-20 shrink-0 text-muted-foreground">取数环境</span>
              <select
                value={environmentId}
                onChange={(e) => setEnvironmentId(e.target.value)}
                className="h-8 w-48 rounded-md border border-input bg-background px-2 text-sm"
              >
                <option value="">
                  默认（{firstEnvironment ? `${firstEnvironment.name}` : '该项目没有环境'}）
                </option>
                {projectEnvironments.map((env) => (
                  <option key={env.id} value={env.id}>
                    {env.name}
                  </option>
                ))}
              </select>
              <span className="text-[11px] text-muted-foreground">
                决定取数时用哪个环境的模块地址与变量。
              </span>
            </label>
          </>
        )}

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

        <div className="flex items-start gap-2 text-xs">
          <span className="w-20 shrink-0 pt-1.5 text-muted-foreground">处理脚本</span>
          <div className="flex flex-1 flex-col gap-1">
            <textarea
              value={transformScript}
              onChange={(e) => setTransformScript(e.target.value)}
              spellCheck={false}
              placeholder={'return pm.data.items.map((x) => x.code).join(",")'}
              className="min-h-20 w-full rounded-md border border-input bg-background px-2 py-1 font-mono text-xs"
            />
            <span className="text-[11px] text-muted-foreground">
              可选。抽取之后跑，用来做 JSONPath 表达不了的加工。脚本拿得到抽取结果（
              <code className="font-mono">pm.data</code>）与原始响应（
              <code className="font-mono">pm.response</code>），用
              <code className="font-mono">return</code> 给出最终值；不写就沿用抽取结果。
            </span>
          </div>
        </div>

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
          <span>取数失败时允许用过期值</span>
        </label>

        <label className="flex items-center gap-2 text-xs">
          <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
          <span>启用（停用后不参与替换）</span>
        </label>

        <div className="mt-1 flex items-center gap-2">
          <Button
            size="sm"
            onClick={() => void handleSubmit()}
            disabled={busy || name.trim() === '' || !hasSource || ownerProjectId === ''}
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
        {saveError && (
          <p className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            {saveError}
          </p>
        )}
      </div>
    </div>
  );
}
