import { useCallback, useEffect, useMemo, useState } from 'react';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { EmptyHint } from '@/components/ui/empty-hint';
import { SettingsCategory } from '../SettingsCategory';
import { useEnvDialogs } from './hooks/useEnvDialogs';
import { EnvBody } from './rows/EnvBody';
import { EnvSwitcher } from './rows/EnvSwitcher';
import type { Environment, ResourceNode } from '@/types/domain';
import { pickDefaultEnvironment } from '@/lib/env-default';

export interface EnvironmentSectionProps {
  // ---- scope ----
  environments: Environment[];
  projectId: string;
  /**
   * 本项目的模块，已由宿主过滤并按 `sortOrder` 排序。它们是模块地址表的行轴，故
   * 没有模块的项目无从配置，表格会说明这一点而不是渲染一张空网格。
   */
  modules: readonly ResourceNode[];

  // ---- env-scoped actions ----
  setEnvironmentVar: (envId: string, key: string, value: string | null) => void;
  /**
   * 停用或启用一个变量。停用的变量**照样存、照样显示，但不参与替换**。
   */
  setEnvironmentVarDisabled: (envId: string, key: string, disabled: boolean) => void;
  renameEnvironmentVar: (envId: string, fromKey: string, toKey: string) => void;
  /**
   * 写一个 (环境 × 模块) 格子里的两个字段：`url` 是模块地址，`proxy` 是代理地址。
   * `null` = 该字段保持原值，`''` = 清空该字段。
   */
  setEnvironmentModuleBaseUrl: (
    envId: string,
    moduleId: string,
    url: string | null,
    proxy: string | null,
  ) => void;
  /** 解析出新环境的 id；创建失败时为 `null`。 */
  addEnvironment: (projectId: string, name: string) => Promise<string | null>;
  deleteEnvironment: (envId: string) => void;
  updateEnvironment: (envId: string, patch: Partial<Environment>) => void;
  /**
   * 本客户端当前用的那个环境（`activeEnvironmentId`）。
   *
   * 面板打开时选中它 —— **前提是它属于本项目**。理由：这一栏改的就是「这个环境在各
   * 模块下的地址与变量」，而请求发出去时用的也是当前环境；两者停在不同的环境上，
   * 用户改的是一处、跑的是另一处。
   *
   * 它不属于本项目时（例如在别的项目的模块标签里看这一栏）就退回默认环境。
   */
  currentEnvironmentId?: string;
}

/**
 * 新建环境的默认名称。
 *
 * 从项目已有的名字推导，保证生成的名字唯一，且与本面板的中文口吻一致——在一个靠名字
 * 指认环境的分区里，两个同名标签无法区分。
 */
export function nextEnvironmentName(existingNames: readonly string[]): string {
  const taken = new Set(existingNames.map((n) => n.trim()));
  const base = '新环境';
  let candidate = base;
  let n = 1;
  while (taken.has(candidate)) {
    n += 1;
    candidate = `${base} ${n}`;
  }
  return candidate;
}

/**
 * Per-environment configuration.
 *
 * Layout:
 *
 *   ┌── env switcher (also the rename surface) ──────────────┐
 *   │  ☁  dev  ⚡ staging  🚀 prod            + 新建环境       │
 *   ├────────────────────────────────────────────────────────┤
 *   ├──────────── 模块地址 ───────────────────────────────────┤
 *   │  模块         模块地址              代理地址            │
 *   │  🗂 默认模块   http://localhost:4000   （留空即无代理）  │
 *   │  🗂 南通6.0    http://192.168.11.101                       │
 *   ├──────────── 环境变量 ───────────────────────────────────┤
 *   │  { } token  eyJhbGciOi...                       [×]    │
 *   │  { } env    dev                                 [×]    │
 *   │                  + 添加变量                            │
 *   └────────────────────────────────────────────────────────┘
 *
 * 没有按环境的表头行：上方的切换器已经点名了每个环境，几像素之下再把当前环境
 * 重复一遍纯属冗余。重命名随之搬进切换器 —— 见 `EnvSwitcher`。
 *
 * Visually it inherits the new SettingsCategory aesthetic — coloured
 * icon containers, SettingRow-style padded rows, badge trailing
 * controls, hover-revealed delete buttons — but adds an env switcher
 * strip across the top because each env holds its own value overrides.
 *
 * 切换器条持有 `新建环境` 控件，故即使一个环境都没有也要渲染 —— 空态是条下方的一句
 * 提示，绝不替代这条。**模块** 标签为更窄的用途展示同一份环境列表（见
 * `ModuleBaseUrlSection`），故本组件不是该列表的唯一宿主。
 *
 * This file is the shell only: it owns the active-env selection, the
 * delete-confirmation state, and the layout. Every presentational part
 * lives under `rows/` (props only, no store access) and the dialog
 * state lives in `hooks/useEnvDialogs`.
 */
export function EnvironmentSection({
  environments,
  projectId,
  modules,
  setEnvironmentVar,
  setEnvironmentVarDisabled,
  renameEnvironmentVar,
  setEnvironmentModuleBaseUrl,
  addEnvironment,
  deleteEnvironment,
  updateEnvironment,
  currentEnvironmentId,
}: EnvironmentSectionProps) {
  const [activeId, setActiveId] = useState<string | null>(null);
  // 创建进行中，以及（若有）落库后应打开重命名的那个环境 id。
  const [creatingEnv, setCreatingEnv] = useState(false);
  const [pendingRenameEnvId, setPendingRenameEnvId] = useState<string | null>(null);

  // ---- modal state ----
  const { deletingEnv, requestEnvDelete, clearEnvDelete, confirmDeleteEnv } = useEnvDialogs({
    deleteEnvironment,
  });

  // 列表形状一变就回到「本客户端当前用的那个环境」，除非用户已明确选过。否则停在
  // 用户上次选的那一行，免得一次重渲染把正在看的那行拽走。
  //
  // 落点是当前环境而不是默认环境：这一栏改的是「这个环境在各模块下的地址与变量」，
  // 而请求发出去时用的也是当前环境。两者停在不同的环境上，用户改的是一处、跑的
  // 是另一处。当前环境不属于本项目时（看的是别的项目的模块）才退回默认环境。
  useEffect(() => {
    if (environments.length === 0) {
      setActiveId(null);
      return;
    }
    if (activeId && environments.some((e) => e.id === activeId)) return;
    const current = environments.find((e) => e.id === currentEnvironmentId);
    const fallback = current ?? pickDefaultEnvironment(environments, projectId);
    setActiveId(fallback?.id ?? environments[0]!.id);
  }, [environments, projectId, currentEnvironmentId, activeId]);

  const activeEnv = useMemo(
    () => environments.find((e) => e.id === activeId) ?? null,
    [environments, activeId],
  );

  const handleNewEnv = async () => {
    if (creatingEnv) return;
    setCreatingEnv(true);
    try {
      const id = await addEnvironment(
        projectId,
        nextEnvironmentName(environments.map((e) => e.name)),
      );
      // `addEnvironment` 会吞掉 IPC 失败并解析为 `null`；此时什么都不新增、什么都不选中，
      // 条保持用户看到的样子 —— 不产生幽灵标签。
      if (id) {
        setActiveId(id);
        setPendingRenameEnvId(id);
      }
    } finally {
      setCreatingEnv(false);
    }
  };

  // 切换环境或开始一次删除都会放弃待执行的自动重命名 —— 该标志不能跟着用户走到别行。
  const handlePick = (id: string) => {
    setPendingRenameEnvId(null);
    setActiveId(id);
  };

  const handleRequestEnvDelete = (env: Environment) => {
    setPendingRenameEnvId(null);
    requestEnvDelete({ id: env.id, name: env.name });
  };

  const handleRenameEnd = useCallback(() => setPendingRenameEnvId(null), []);

  const handleRenameEnv = useCallback(
    (envId: string, name: string) => updateEnvironment(envId, { name }),
    [updateEnvironment],
  );

  const handleAddVar = () => {
    if (!activeEnv) return;
    // 就地追加一个空行。
    setEnvironmentVar(activeEnv.id, '', '');
  };

  return (
    <SettingsCategory
      hideDivider
      // 用 `stacked`，让每块面板（环境切换器 + 单环境内容）读起来都是自足的界面，
      // 外面不再套一层卡片壳。
      variant="stacked"
    >
      <div className="space-y-3">
        {/* ----- 环境切换器（顶部标签条） ----------------- */}
        {/* 无条件渲染，而这正是重点：末尾的 `新建环境` 按钮就住在这条里，故把这条挂在
            「列表非空」之后，会让零环境的项目只剩一句提示、没有创建第一个环境的途径。 */}
        <EnvSwitcher
          environments={environments}
          activeId={activeId}
          onPick={handlePick}
          onRequestDelete={handleRequestEnvDelete}
          onAdd={handleNewEnv}
          addDisabled={creatingEnv}
          // 重命名现在住在切换器上 —— 见 `EnvSwitcher`。自动打开的闩与原先表头行用的是
          // 同一个，故「新建并命名」仍是一个动作。
          onRename={handleRenameEnv}
          autoRenameId={pendingRenameEnvId}
          onAutoRenameEnd={handleRenameEnd}
        />

        {environments.length === 0 ? (
          <EmptyHint className="rounded-lg border-border bg-card/30 px-3 py-8 text-xs">
            该项目还没有环境 · 点「新建环境」创建第一个，用来切换模块地址与环境变量
          </EmptyHint>
        ) : (
          activeEnv && (
            <div className="overflow-hidden rounded-lg border border-border/70 bg-card/40 shadow-[0_1px_0_0_rgba(255,255,255,0.02)_inset]">
              {/* 以 env id 为 key，切换标签即重置内容自身的临时状态 —— 上一个环境里编辑到
                  一半的输入框、自动重命名的闩，都不会串到这边来。 */}
              <EnvBody
                key={activeEnv.id}
                env={activeEnv}
                modules={modules}
                setEnvironmentVar={setEnvironmentVar}
                setEnvironmentVarDisabled={setEnvironmentVarDisabled}
                renameEnvironmentVar={renameEnvironmentVar}
                setEnvironmentModuleBaseUrl={setEnvironmentModuleBaseUrl}
                onAddVar={handleAddVar}
              />
            </div>
          )
        )}
      </div>

      {/* ---------- Dialogs ---------- */}

      <ConfirmDialog
        open={!!deletingEnv}
        onOpenChange={(o) => !o && clearEnvDelete()}
        title={`删除环境「${deletingEnv?.name ?? ''}」?`}
        description="依赖它的接口会停止使用这个环境的模块地址。"
        confirmText="删除"
        destructive
        onConfirm={confirmDeleteEnv}
      />
    </SettingsCategory>
  );
}
