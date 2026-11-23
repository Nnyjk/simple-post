/**
 * 动态变量定义的表单 —— 新建与编辑共用一份。
 *
 * 数据源二选一：引用树里的一个接口（那个接口自己的 base URL、鉴权与前置脚本都会生效），或
 * 一份内联请求（method / url / headers / body / auth 的 JSON 文本，URL 必须写完整）。
 *
 * 引用接口时，表单还会给出**取数用的环境**，两态：
 *
 * - **继承当前环境**（默认，值为空）：发送请求时用户选中的那个环境就用哪个。同一个
 *   `{{name}}` 于是随用户切环境换值。
 * - **指定环境**：固定用某个环境。它必须属于数据源接口所属项目——环境那一格是按
 *   (环境 × 模块) 寻址的，别的项目的环境会读到不属于这个模块的格子。
 *
 * 定义挂在**项目**里，故表单要知道它属于哪个项目：新建时是当前项目，编辑时是这条定义自己的
 * 归属项目。变量名在项目内唯一。
 *
 * 表单内容不在组件里：它们存在 store 的 `dynamicDrafts[项目 id]`（见
 * `stores/dynamic-slice.ts`），因为切到别的标签时面板连同这张表单一起被 `WorkspaceShell`
 * 卸载，组件 state 留不住用户填了一半的内容。组件只留 `busy` 这类瞬时状态。
 */
import { useMemo, useRef, useState } from 'react';
import { useAppStore } from '@/stores/app-store';
import {
  creatingDraftKey,
  emptyDynamicVariableDraft,
  type DynamicVariableDraft,
} from '@/stores/dynamic-slice';
import type { DynamicVariable, DynamicVariableView, ValueMode } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Select } from '@/components/ui/select';
import { InlineError } from '@/components/ui/inline-error';
import { EndpointTreePicker } from './EndpointTreePicker';
import { ExtractPreview } from './ExtractPreview';
import { buildEditPatch, keepsSource, ttlSecondsOf } from './edit-patch';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';

export interface DynamicVariableFormProps {
  /** 传了就走编辑模式，不传就是新建。 */
  variable?: DynamicVariableView;
  /**
   * 归属项目。新建时表单把它写进 `projectId`；编辑时表单优先用 `variable.projectId`，
   * 那个值缺失时用这个 prop，两个都缺时用当前项目。
   */
  projectId?: string;
  onCancel: () => void;
  /**
   * 保存成功。带上**提交的那一份**草稿与落库的那份定义：调用方据此判断期间有没有新输入，
   * 并在有输入时把草稿改写成刚落地那条的编辑态。
   */
  onDone: (
    submitted: DynamicVariableDraft,
    saved: DynamicVariable,
    patch: Partial<DynamicVariable>,
  ) => void;
}

/** 没有草稿时的回退：面板只在草稿存在时挂载表单，故这一份不会出现在正常路径上。 */
const EMPTY_DRAFT = emptyDynamicVariableDraft();

/** 表单行的骨架：左列是字段标签（固定 5rem，各行标签对齐），右列占满剩余宽度。八行共用
 * 一份，改宽度或列间距时不必逐行同步。 */
const FORM_ROW = 'grid grid-cols-[5rem_minmax(0,1fr)] items-start gap-x-2 gap-y-1 text-xs';

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

  // 这次保存正在路上。标志在 store 里而不是组件 state：卡片换环境会重挂，切标签还会卸载整张
  // 表单，`busy` 都随之归零，而请求还在路上——那时再点一次，编辑会多发一个 PATCH（乱序落地
  // 就盖掉新编辑），新建会落成两条定义（撞项目内名字唯一）。新建没有定义 id 可用，按键是
  // `creatingDraftKey(项目)`，故切回来仍是「保存中」。
  const savingThis = useAppStore((s) =>
    variable !== undefined
      ? s.savingDynamicVariableIds.has(variable.id)
      : s.savingDynamicVariableIds.has(creatingDraftKey(ownerProjectId)),
  );

  // 表单内容全在 store 的草稿里，按项目分桶。选择器只读当前项目那一份对象：别的项目改草稿
  // 换的是记录里的另一个键，这里的引用不动，故不会触发重渲染。
  const draft = useAppStore((s) => s.dynamicDrafts[ownerProjectId]) ?? EMPTY_DRAFT;
  const patchDraft = useAppStore((s) => s.patchDynamicVariableDraft);
  const {
    name,
    description,
    sourceKind,
    sourceEndpointId,
    sourceProjectId,
    sourceEndpointName,
    sourceInlineJson,
    environmentId,
    extractPath,
    transformScript,
    valueMode,
    ttlSeconds,
    staleFallback,
    enabled,
  } = draft;

  const [busy, setBusy] = useState(false);
  const formRef = useRef<HTMLDivElement>(null);

  // 只在这一份表单挂载时播一次：依赖留空，用户每次敲键都重渲染，但动画不复播。
  useGSAP(
    () =>
      motionSafe(() =>
        gsap.fromTo(
          formRef.current,
          { opacity: 0, y: 4 },
          { opacity: 1, y: 0, duration: 0.2, ease: 'power2.out', clearProps: 'transform' },
        ),
      ),
    { scope: formRef },
  );

  // 「数据源接口已被删除」的定义落在**引用接口**形态：那条路是唯一能救它的（重新挑一个
  // 接口），而内联形态会让用户以为要手写一份请求。
  const sourceDeleted = !!variable && !variable.sourceEndpointId && !variable.sourceInlineJson;
  const usesEndpoint = sourceKind === 'endpoint';

  // 数据源接口所属项目里的环境，按名字排序。选哪一格是用户的事，故给一份稳定的次序：
  // 名字相同的按 id 次序排列，这样同一个项目两次打开表单的顺序一致。
  const projectEnvironments = useMemo(
    () =>
      environments
        .filter((e) => e.projectId === sourceProjectId)
        .sort((a, b) => (a.name === b.name ? cmp(a.id, b.id) : cmp(a.name, b.name))),
    [environments, sourceProjectId],
  );

  const handleSelectEndpoint = (endpointId: string, projectId: string, endpointName: string) => {
    // 一次写完四格：中途的半成品（接口变了、环境还是上一个接口的）会去取错环境的值。
    patchDraft(ownerProjectId, {
      sourceKind: 'endpoint',
      sourceEndpointId: endpointId,
      sourceProjectId: projectId,
      sourceEndpointName: endpointName,
      environmentId: '',
    });
  };

  // 没有数据源的定义永远解析不出值，故新建时不让人存：引用形态要选了接口，内联形态要填了
  // 请求。
  //
  // 编辑时判的是**将要提交的那份 patch 会不会把来源清空**，不是「定义此刻有没有来源」：
  // 别的客户端把删掉的接口补回来之后，本地只改名字的编辑不该因为「我这边的列表还没有来源」
  // 就被禁掉——那份 patch 里根本没有来源那两格，存下去不弄丢任何来源。反过来，把两个来源
  // 都清空的那份 patch 必须挡下。
  const hasSource = usesEndpoint ? sourceEndpointId !== '' : sourceInlineJson.trim() !== '';

  /**
   * 差量比对以**打开表单时那份定义的快照**为准，不以「此刻的 `variable`」为准。
   *
   * 快照随草稿一起存在 store 里（`draft.openedWith`）：表单可能被卸载重挂（切标签），列表也
   * 可能被刷新（切环境、别处保存都会触发重载）。拿刷新后的定义当基线，会把别人刚存的改动
   * 判成「我改的」再写回去——两个窗口各改一格时互相覆盖。
   */
  const openedWith = draft.openedWith;
  const baseline = openedWith ?? variable;
  const sourceIsAcceptable =
    hasSource || (baseline !== undefined && keepsSource(baseline, buildEditPatch(baseline, draft)));

  const handleSubmit = async () => {
    setBusy(true);
    try {
      const ttlValue = ttlSecondsOf(ttlSeconds);
      let saved: DynamicVariable | null = null;
      // 发出去的那份 patch：收尾时按它决定推进基线的哪几格（没提到的格保持原基线）。
      let patch: Partial<DynamicVariable> = {};
      if (variable) {
        // `baseline` 缺失时回退到此刻这条定义：草稿是被 `patchDynamicVariableDraft` 凭空垫出来的那种
        // 极端情形，也不能把「编辑」错走成「新建」。
        patch = buildEditPatch(baseline ?? variable, draft);
        saved = await updateDynamicVariable(variable.id, patch);
        // 保存失败时表单留着：用户填的内容不丢。
        if (!saved) return;
      } else {
        saved = await createDynamicVariable({
          projectId: ownerProjectId,
          name,
          description: description || undefined,
          sourceEndpointId: usesEndpoint ? sourceEndpointId : null,
          sourceInlineJson: usesEndpoint ? null : sourceInlineJson || null,
          sourceEnvironmentId: usesEndpoint ? environmentId || null : null,
          extractPath,
          // 只写空白的脚本按「没有脚本」提交：与编辑分支的归一保持一致。
          transformScript: transformScript.trim() === '' ? null : transformScript,
          valueMode,
          ttlSeconds: ttlValue,
          staleFallback,
          enabled,
        });
        if (!saved) return;
      }
      onDone(draft, saved, patch);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      ref={formRef}
      className="flex flex-col divide-y divide-border rounded-lg border border-border"
    >
      <div className="px-4 py-3">
        <h3 className="text-xs font-medium">{variable ? '编辑定义' : '新建定义'}</h3>
        <p className="mt-1 text-micro text-muted-foreground">
          归属项目：{projects.find((p) => p.id === ownerProjectId)?.name ?? '（未知）'}
        </p>
      </div>

      <section className="flex flex-col gap-2 px-4 py-3">
        <h4 className="text-micro font-medium text-muted-foreground">基本</h4>
        <label className={FORM_ROW}>
          <span className="text-muted-foreground">变量名</span>
          <div className="flex min-w-0 flex-col gap-1">
            <Input
              value={name}
              onChange={(e) => patchDraft(ownerProjectId, { name: e.target.value })}
              placeholder="sns"
              className="w-full min-w-0 font-mono"
            />
            <span className="text-micro text-faint">只能用字母、数字与下划线。</span>
          </div>
        </label>

        <label className={FORM_ROW}>
          <span className="text-muted-foreground">说明</span>
          <Input
            value={description}
            onChange={(e) => patchDraft(ownerProjectId, { description: e.target.value })}
            placeholder="站点清单"
            className="w-full min-w-0"
          />
        </label>
      </section>

      <section className="flex min-w-0 flex-col gap-2 px-4 py-3">
        <h4 className="text-micro font-medium text-muted-foreground">数据源</h4>
        <div className={FORM_ROW}>
          <span className="pt-1.5 text-muted-foreground">形态</span>
          <div className="flex min-w-0 flex-col gap-1">
            <Select
              value={sourceKind}
              onChange={(e) =>
                patchDraft(ownerProjectId, {
                  sourceKind: e.target.value as 'endpoint' | 'inline',
                })
              }
              className="w-full min-w-0"
              options={[
                { value: 'endpoint', label: '引用接口' },
                { value: 'inline', label: '内联请求' },
              ]}
            />
            {usesEndpoint && sourceEndpointId && (
              <span className="text-micro text-faint">
                已选接口 {sourceEndpointName || sourceEndpointId}
              </span>
            )}
            {sourceDeleted && (
              <p className="text-micro text-destructive">
                这条定义引用的接口已被删除，故它现在没有数据源。挑一个接口替上它即可救回这条定义。
              </p>
            )}
            {usesEndpoint && (
              <EndpointTreePicker value={sourceEndpointId} onSelect={handleSelectEndpoint} />
            )}
            {!usesEndpoint && (
              <textarea
                value={sourceInlineJson}
                onChange={(e) => patchDraft(ownerProjectId, { sourceInlineJson: e.target.value })}
                spellCheck={false}
                placeholder='{"method":"GET","url":"https://example.test/points","headers":[],"body":{"mode":"none","content":""},"auth":{"kind":"none"}}'
                className="min-h-20 w-full max-w-full min-w-0 rounded-md border border-input bg-background px-2 py-1 font-mono text-xs whitespace-pre-wrap break-all placeholder:text-faint placeholder:italic"
              />
            )}
          </div>
        </div>

        {usesEndpoint && sourceEndpointId !== '' && (
          <>
            <label className={FORM_ROW}>
              <span className="text-muted-foreground">取数环境</span>
              <div className="flex min-w-0 flex-col gap-1">
                <Select
                  value={environmentId}
                  onChange={(e) => patchDraft(ownerProjectId, { environmentId: e.target.value })}
                  className="w-full min-w-0"
                  options={[
                    { value: '', label: '继承当前环境' },
                    ...projectEnvironments.map((env) => ({
                      value: env.id,
                      label: `指定：${env.name}`,
                    })),
                  ]}
                />
                <span className="text-micro text-faint">
                  {environmentId === ''
                    ? '取数时用你发送请求时选中的那个环境；你切环境，它的值就跟着变。没有可用环境时，用这个接口所属项目的默认环境（最早建的那个）。'
                    : '固定用这个环境。'}
                </span>
              </div>
            </label>
          </>
        )}
      </section>

      <section className="flex min-w-0 flex-col gap-2 px-4 py-3">
        <h4 className="text-micro font-medium text-muted-foreground">抽取与处理</h4>
        <label className={FORM_ROW}>
          <span className="text-muted-foreground">抽取规则</span>
          <div className="flex min-w-0 flex-col gap-1">
            <Input
              value={extractPath}
              onChange={(e) => patchDraft(ownerProjectId, { extractPath: e.target.value })}
              placeholder="$"
              className="w-full min-w-0 font-mono"
            />
            <span className="text-micro text-faint">JSONPath；`$` 表示整段响应。</span>
          </div>
        </label>

        {/* 预览夹在「抽取规则」与「处理脚本」之间：它产出的两样东西就是这两个字段
            （点节点写抽取规则，生成片段写处理脚本）。 */}
        <ExtractPreview
          usesEndpoint={usesEndpoint}
          sourceEndpointId={sourceEndpointId}
          sourceEndpointName={sourceEndpointName}
          sourceInlineJson={sourceInlineJson}
          environmentId={environmentId}
          extractPath={extractPath}
          transformScript={transformScript}
          onPickPath={(path) => patchDraft(ownerProjectId, { extractPath: path })}
          onUseScript={(script) => patchDraft(ownerProjectId, { transformScript: script })}
        />

        <div className={FORM_ROW}>
          <span className="pt-1.5 text-muted-foreground">处理脚本</span>
          <div className="flex min-w-0 flex-col gap-1">
            <textarea
              value={transformScript}
              onChange={(e) => patchDraft(ownerProjectId, { transformScript: e.target.value })}
              spellCheck={false}
              placeholder={'return pm.data.items.map((x) => x.code).join(",")'}
              className="min-h-20 w-full max-w-full min-w-0 rounded-md border border-input bg-background px-2 py-1 font-mono text-xs placeholder:text-faint placeholder:italic"
            />
            <span className="text-micro text-faint">
              可选。抽取之后跑，用来做 JSONPath 表达不了的加工。脚本拿得到抽取结果（
              <code className="font-mono">pm.data</code>）与原始响应（
              <code className="font-mono">pm.response</code>），用
              <code className="font-mono">return</code> 给出最终值；不写就沿用抽取结果。
            </span>
          </div>
        </div>
      </section>

      <section className="flex flex-col gap-2 px-4 py-3">
        <h4 className="text-micro font-medium text-muted-foreground">缓存与开关</h4>
        <label className={FORM_ROW}>
          <span className="text-muted-foreground">值的形态</span>
          <div className="flex min-w-0 flex-col gap-1">
            <Select
              value={valueMode}
              onChange={(e) =>
                patchDraft(ownerProjectId, { valueMode: e.target.value as ValueMode })
              }
              className="w-full min-w-0"
              options={[
                { value: 'auto', label: 'auto' },
                { value: 'json', label: 'json' },
                { value: 'text', label: 'text' },
              ]}
            />
            <span className="text-micro text-faint">
              auto 与 text 输出相同；json 会给标量加引号。
            </span>
          </div>
        </label>

        <label className={FORM_ROW}>
          <span className="text-muted-foreground">缓存时长</span>
          <div className="flex min-w-0 flex-col gap-1">
            <Input
              type="number"
              min={0}
              value={ttlSeconds}
              onChange={(e) => patchDraft(ownerProjectId, { ttlSeconds: e.target.value })}
              className="w-full min-w-0"
            />
            <span className="text-micro text-faint">秒。</span>
          </div>
        </label>

        <label className="flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={staleFallback}
            onChange={(e) => patchDraft(ownerProjectId, { staleFallback: e.target.checked })}
          />
          <span>取数失败时允许用过期值</span>
        </label>

        <label className="flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => patchDraft(ownerProjectId, { enabled: e.target.checked })}
          />
          <span>启用（停用后不参与替换）</span>
        </label>
      </section>

      <section className="flex flex-col gap-2 px-4 py-3">
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            onClick={() => void handleSubmit()}
            disabled={
              busy ||
              savingThis ||
              name.trim() === '' ||
              !sourceIsAcceptable ||
              ownerProjectId === ''
            }
          >
            {savingThis || busy ? '保存中…' : '保存'}
          </Button>
          <Button variant="ghost" size="sm" onClick={onCancel}>
            取消
          </Button>
        </div>
        {saveError && <InlineError className="px-3 py-2 text-xs">{saveError}</InlineError>}
      </section>
    </div>
  );
}
