/**
 * 动态变量面板 —— 项目内的一页。
 *
 * 一条动态变量是一份声明：变量名 → 数据源 → 抽取规则 → 缓存时长。消费端点的请求体里写
 * `{{sns}}`，发送时程序取数、抽取、缓存并替换。这个面板负责声明它、显示它当前的值、并在
 * 取数失败时把原因说清楚。
 *
 * 定义**挂在项目里**：变量名在项目内唯一，不同项目可以有同名变量而互不覆盖。故这个面板只列
 * **当前项目**的定义。
 *
 * 面板读的是 store 里的 `dynamicVariables`（列表命令一次带回定义 + 当前值 + 缓存状态），故它
 * 不自己发请求；取数与写覆盖值都走 store 动作。
 */
import { useCallback, useEffect, useMemo, useRef } from 'react';
import { useAppStore } from '@/stores/app-store';
import type { DynamicVariableView } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { EmptyHint } from '@/components/ui/empty-hint';
import { InlineError } from '@/components/ui/inline-error';
import { LoadingState } from '@/components/ui/loading-indicator';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';
import { dynamicListMatchesEnvironment } from '@/stores/dynamic-slice';
import { DynamicVariableCard } from './DynamicVariableCard';
import { DynamicVariableForm } from './DynamicVariableForm';

export function DynamicVariablesPanel() {
  const projectId = useAppStore((s) => s.activeProjectId);
  // 列表带回的当前值是「在某个环境下」的值，而继承取数环境的定义按用户选中的环境取值：
  // 切了环境，这个列表就得重载，否则卡片上的值还是上一个环境的。
  const activeEnvironmentId = useAppStore((s) => s.activeEnvironmentId);
  const loaded = useAppStore((s) => s.dynamicVariables);
  // 这批列表是「在哪个 (项目 × 环境) 下取到的」。首载时它是 null：此刻列表还没回来，
  // 面板不能先说「还没有动态变量」。
  const listScope = useAppStore((s) => s.dynamicVariablesScope);
  const resolving = useAppStore((s) => s.dynamicInFlight > 0);
  // 列表请求本身还在路上。只看条数判断不了：请求失败时列表不会再来，加载态得跟着请求收。
  const pending = useAppStore((s) => s.dynamicListPending);
  const error = useAppStore((s) => s.dynamicError);
  const loadDynamicVariables = useAppStore((s) => s.loadDynamicVariables);
  const refreshAllDynamicVariables = useAppStore((s) => s.refreshAllDynamicVariables);
  const setDynamicOverride = useAppStore((s) => s.setDynamicOverride);
  const refreshDynamicVariable = useAppStore((s) => s.refreshDynamicVariable);
  const deleteDynamicVariable = useAppStore((s) => s.deleteDynamicVariable);
  // 面板的两态（在新建 / 在编辑哪一条）与表单内容都在 store 的草稿里：切到别的标签时这个
  // 面板会被 `WorkspaceShell` 卸载，组件 state 留不住它们。草稿按项目分桶。
  const draft = useAppStore((s) => s.dynamicDrafts[projectId]);
  const beginCreate = useAppStore((s) => s.beginCreateDynamicVariableDraft);
  const beginEdit = useAppStore((s) => s.beginEditDynamicVariableDraft);
  const clearDraft = useAppStore((s) => s.clearDynamicVariableDraft);
  const finishSave = useAppStore((s) => s.finishDynamicVariableSave);

  const creating = draft?.creating ?? false;
  const editingId = draft?.editingId ?? null;
  const listRef = useRef<HTMLDivElement>(null);

  // 只列当前项目的定义：别的项目的同名变量不归这条标签管。
  const variables = useMemo(
    () => loaded.filter((v) => v.projectId === projectId),
    [loaded, projectId],
  );

  // 列表还没回到当前这一格：显示加载态，别先给一句「还没有动态变量」。
  const listMatches = dynamicListMatchesEnvironment(
    listScope,
    projectId,
    activeEnvironmentId || null,
  );
  const loading = !listMatches && pending;
  // 请求已经收场、这一格仍没有列表：那是一次失败的读取，不是「还没有变量」。原因由上方
  // 那条 `InlineError` 说，这里只说明这一屏缺的是什么。
  const readFailed = !listMatches && !pending && variables.length === 0 && error !== null;

  useEffect(() => {
    void loadDynamicVariables(projectId);
  }, [loadDynamicVariables, projectId, activeEnvironmentId]);

  // 依赖是「项目、取数环境、条数」：换项目、切环境、以及条数一变（含首批数据到达）都会整体
  // 重播一次这一轮淡入。切环境时卡片会按新 key 重挂，条数不变也得重播，否则新卡片直接出现在
  // 终态。`revertOnUpdate` 让上一份上下文连同它捕获的节点一起释放。
  // 错峰用的是固定总预算（amount），无论多少条都在四分之一秒内摊完，条数再多也不会把
  // 末项拖到一秒之后。
  useGSAP(
    () => {
      const cards = listRef.current?.querySelectorAll('[data-dynamic-card]');
      if (!cards || cards.length === 0) return;
      return motionSafe(() =>
        gsap.fromTo(
          cards,
          { opacity: 0, y: 4 },
          {
            opacity: 1,
            y: 0,
            duration: 0.2,
            ease: 'power2.out',
            stagger: { amount: 0.25 },
            clearProps: 'transform',
          },
        ),
      );
    },
    {
      scope: listRef,
      dependencies: [projectId, activeEnvironmentId, variables.length],
      revertOnUpdate: true,
    },
  );

  const handleResolveAll = useCallback(async () => {
    // 语义在 store 里：这条动作强制重取（跳过缓存）并重载列表。面板只负责触发。
    await refreshAllDynamicVariables(projectId);
  }, [refreshAllDynamicVariables, projectId]);

  const handleRefresh = useCallback(
    async (variable: DynamicVariableView) => {
      await refreshDynamicVariable(variable.id);
    },
    [refreshDynamicVariable],
  );

  const handleClearOverride = useCallback(
    async (variable: DynamicVariableView) => {
      await setDynamicOverride(variable.id, null);
    },
    [setDynamicOverride],
  );

  return (
    <div className="h-full min-h-0 overflow-y-auto px-4 pb-8">
      <div className="mx-auto flex max-w-3xl flex-col gap-4 pt-4">
        <header className="flex items-center gap-2">
          <h2 className="text-sm font-medium">动态变量</h2>
          <span className="text-xs text-muted-foreground">
            在请求里写 {'{{变量名}}'}，发送时自动取数并替换。变量名在本项目内唯一。
          </span>
          <div className="ml-auto flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              // 这一批还在取数时再点一次只是把同一批请求叠上去；结果正确（世代号挡掉旧的），
              // 但没必要付两次取数的代价。
              disabled={resolving}
              onClick={() => void handleResolveAll()}
            >
              全部刷新
            </Button>
            <Button size="sm" onClick={() => beginCreate(projectId)}>
              新建定义
            </Button>
          </div>
        </header>

        {error && <InlineError className="px-3 py-2 text-xs">{error}</InlineError>}

        {loading && <LoadingState className="py-6" label="正在读取动态变量…" />}

        {!loading && variables.length === 0 && !creating && listMatches && (
          <EmptyHint className="py-6 text-xs">
            还没有动态变量。新建一条：给它一个名字、一个数据源（引用树里的接口，或一份内联请求）、
            一条 JSONPath 抽取规则与一个缓存时长。
          </EmptyHint>
        )}

        {readFailed && (
          <p className="text-micro text-muted-foreground">暂时读不到这个项目的动态变量</p>
        )}

        <div ref={listRef} aria-busy={pending} className="flex flex-col gap-2">
          {variables.map((variable) => (
            <div key={`${variable.id}:${activeEnvironmentId}`} data-dynamic-card>
              <DynamicVariableCard
                // 卡片自带覆盖值编辑器的状态，而值是 (定义 × 取数环境) 的一格：换环境就是另一格，
                // 卡片必须跟着重挂，否则 A 环境那行文本会跟着写进 B 环境。切环境时整个面板都重载了
                // 列表，重挂这一次不额外付代价。
                variable={variable}
                projectId={projectId}
                resolving={resolving}
                editing={editingId === variable.id}
                onEdit={() => beginEdit(variable)}
                onCancelEdit={() => clearDraft(projectId)}
                onFinishEdit={(submitted, saved, patch) =>
                  finishSave(projectId, submitted, saved, patch)
                }
                onRefresh={() => void handleRefresh(variable)}
                onClearOverride={() => void handleClearOverride(variable)}
                onDelete={() => void deleteDynamicVariable(variable.id)}
              />
            </div>
          ))}
        </div>

        {creating && (
          <DynamicVariableForm
            projectId={projectId}
            onCancel={() => clearDraft(projectId)}
            onDone={(submitted, saved, patch) => finishSave(projectId, submitted, saved, patch)}
          />
        )}
      </div>
    </div>
  );
}
