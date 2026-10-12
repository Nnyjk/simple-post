import { useCallback, useEffect, useMemo, useState } from 'react';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { EmptyHint } from '@/components/ui/empty-hint';
import { SettingsCategory } from '../SettingsCategory';
import { useEnvDialogs } from './hooks/useEnvDialogs';
import { EnvBody } from './rows/EnvBody';
import { EnvSwitcher } from './rows/EnvSwitcher';
import type { BaseUrlDefinition, Environment, ResourceNode } from '@/types/domain';

export interface EnvironmentSectionProps {
  // ---- scope ----
  environments: Environment[];
  projectId: string;
  /**
   * This project's modules, already filtered and `sortOrder`-sorted by the
   * host. They are the row axis of the 前置 URL table: the row set is
   * (every module × every project service), so a project with no modules
   * has nothing to configure and the table says so instead of rendering an
   * empty grid.
   */
  modules: readonly ResourceNode[];

  // ---- env-scoped actions ----
  setEnvironmentVar: (envId: string, key: string, value: string | null) => void;
  renameEnvironmentVar: (envId: string, fromKey: string, toKey: string) => void;
  /**
   * Set (or with `null` clear) one cell of the value layer: what service
   * `defId` points at, for module `moduleId`, in this environment.
   */
  setEnvironmentModuleBaseUrl: (
    envId: string,
    defId: string,
    moduleId: string,
    url: string | null,
  ) => void;
  /** Resolves the new env's id, or `null` when the create failed. */
  addEnvironment: (projectId: string, name: string) => Promise<string | null>;
  deleteEnvironment: (envId: string) => void;
  updateEnvironment: (envId: string, patch: Partial<Environment>) => void;

  // ---- project-scoped service definitions ----
  baseUrlDefinitions: readonly BaseUrlDefinition[];
  /** The project's ★. Project-wide — there is no per-module default. */
  defaultBaseUrlDefinitionId: string | null;
  addBaseUrlDefinition: (projectId: string, name: string) => string;
  updateBaseUrlDefinition: (
    projectId: string,
    defId: string,
    patch: Partial<Pick<BaseUrlDefinition, 'name' | 'description'>>,
  ) => void;
  deleteBaseUrlDefinition: (projectId: string, defId: string) => void;
  /** Star a service as the project default, or pass `null` to unstar. */
  setDefaultBaseUrlDefinition: (projectId: string, defId: string | null) => void;
}

/**
 * Default name for a freshly created environment.
 *
 * This used to be the hardcoded literal `'new-env'`, so every env a
 * project owned beyond the first was born with the same name — in a
 * section where environments are addressed by name, two tabs reading
 * `new-env` are indistinguishable. Deriving from the names already in
 * the project keeps the generated label unique and in the panel's
 * Chinese voice (the same reasoning gives baseUrl definitions their
 * `新服务` default).
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
 *   ┌── env tab strip ─────────────────────────────────────────┐
 *   │  ☁  dev  ⚡ staging  🚀 prod            + 新建环境       │
 *   ├────────────────────────────────────────────────────────┤
 *   │  dev                                              [active]
 *   ├──────────── 前置 URL ───────────────────────────────────┤
 *   │  模块         服务          前置 URL                     │
 *   │  🗂 默认模块   ★ 默认服务   http://localhost:4000/demo  │
 *   │               数据服务     http://127.0.0.1:9100/api   │
 *   │               + 添加服务                                 │
 *   │  🗂 南通6.0    ★ 默认服务     http://192.168.11.101:27000 │
 *   ├──────────── 环境变量 ───────────────────────────────────┤
 *   │  { } token  eyJhbGciOi...                       [×]    │
 *   │  { } env    dev                                 [×]    │
 *   │                  + 添加变量                            │
 *   └────────────────────────────────────────────────────────┘
 *
 * Visually it inherits the new SettingsCategory aesthetic — coloured
 * icon containers, SettingRow-style padded rows, badge trailing
 * controls, hover-revealed delete buttons — but adds an env switcher
 * strip across the top because each env holds its own value overrides.
 *
 * The 前置 URL table is grouped by module because the **address** is
 * per module: every module group lists the project's whole service list and
 * fills in its own cells. The ★ is per project, not per module.
 *
 * The switcher strip is the one home of the `新建环境` control, so it
 * renders even with zero environments — the empty state is a hint under
 * the strip, never a replacement for it.
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
  renameEnvironmentVar,
  setEnvironmentModuleBaseUrl,
  addEnvironment,
  deleteEnvironment,
  updateEnvironment,
  baseUrlDefinitions,
  defaultBaseUrlDefinitionId,
  addBaseUrlDefinition,
  updateBaseUrlDefinition,
  deleteBaseUrlDefinition,
  setDefaultBaseUrlDefinition,
}: EnvironmentSectionProps) {
  const [activeId, setActiveId] = useState<string | null>(null);
  // Create is in flight, and the env (if any) whose name should be
  // opened for rename once it lands.
  const [creatingEnv, setCreatingEnv] = useState(false);
  const [pendingRenameEnvId, setPendingRenameEnvId] = useState<string | null>(null);

  // ---- modal state ----
  const {
    deletingBaseUrl,
    deletingEnv,
    requestBaseUrlDelete,
    requestEnvDelete,
    clearBaseUrlDelete,
    clearEnvDelete,
    confirmDeleteBaseUrl,
    confirmDeleteEnv,
  } = useEnvDialogs({ projectId, deleteBaseUrlDefinition, deleteEnvironment });

  // Default to the project-active env whenever the list shape changes,
  // unless the user has explicitly picked one. Otherwise stay on the
  // last selection so a re-render doesn't yank the active tab.
  useEffect(() => {
    if (environments.length === 0) {
      setActiveId(null);
      return;
    }
    if (activeId && environments.some((e) => e.id === activeId)) return;
    const active = environments.find((e) => e.isActive);
    setActiveId(active?.id ?? environments[0]!.id);
  }, [environments, activeId]);

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
      // `addEnvironment` swallows the IPC failure and resolves `null`;
      // in that case nothing is added, nothing is selected, and the
      // strip is left exactly as the user saw it — no phantom tab.
      if (id) {
        setActiveId(id);
        setPendingRenameEnvId(id);
      }
    } finally {
      setCreatingEnv(false);
    }
  };

  // Switching envs or arming a delete abandons a pending auto-rename:
  // the flag must not follow the user to a different row.
  const handlePick = (id: string) => {
    setPendingRenameEnvId(null);
    setActiveId(id);
  };

  const handleRequestEnvDelete = (env: Environment) => {
    setPendingRenameEnvId(null);
    requestEnvDelete({ id: env.id, name: env.name });
  };

  const handleRenameEnd = useCallback(() => setPendingRenameEnvId(null), []);

  const handleAddVar = () => {
    if (!activeEnv) return;
    // Inline empty-row append — same as the old card-based UX.
    setEnvironmentVar(activeEnv.id, '', '');
  };

  return (
    <SettingsCategory
      hideDivider
      // `stacked` so each panel (env switcher + per-env body) reads as
      // its own self-contained surface — same as before, no extra card
      // shell around everything.
      variant="stacked"
    >
      <div className="space-y-3">
        {/* ----- Env switcher (top tab strip) ----------------- */}
        {/* Unconditional, and that is the whole point: the trailing
            `新建环境` button lives in this strip, so gating the strip
            behind a non-empty list left a project with zero envs
            showing a bare hint string and no way to create its first
            environment. */}
        <EnvSwitcher
          environments={environments}
          activeId={activeId}
          onPick={handlePick}
          onRequestDelete={handleRequestEnvDelete}
          onAdd={handleNewEnv}
          addDisabled={creatingEnv}
        />

        {environments.length === 0 ? (
          <EmptyHint className="rounded-lg border-border bg-card/30 px-3 py-8 text-xs">
            该项目还没有环境 · 点「新建环境」创建第一个，用来切换前置 URL 与环境变量
          </EmptyHint>
        ) : (
          activeEnv && (
            <div className="overflow-hidden rounded-lg border border-border/70 bg-card/40 shadow-[0_1px_0_0_rgba(255,255,255,0.02)_inset]">
              {/* Keyed by env id so switching tabs resets the body's own
                  transient state — an input left mid-edit on the previous
                  env, and the auto-rename latch, cannot bleed across. */}
              <EnvBody
                key={activeEnv.id}
                env={activeEnv}
                projectId={projectId}
                modules={modules}
                setEnvironmentVar={setEnvironmentVar}
                renameEnvironmentVar={renameEnvironmentVar}
                setEnvironmentModuleBaseUrl={setEnvironmentModuleBaseUrl}
                updateEnvironment={updateEnvironment}
                onAddVar={handleAddVar}
                baseUrlDefinitions={baseUrlDefinitions}
                defaultBaseUrlDefinitionId={defaultBaseUrlDefinitionId}
                addBaseUrlDefinition={addBaseUrlDefinition}
                updateBaseUrlDefinition={updateBaseUrlDefinition}
                setDefaultBaseUrlDefinition={setDefaultBaseUrlDefinition}
                onDeleteBaseUrl={requestBaseUrlDelete}
                autoRename={pendingRenameEnvId === activeEnv.id}
                onRenameEnd={handleRenameEnd}
              />
            </div>
          )
        )}
      </div>

      {/* ---------- Dialogs ---------- */}

      <ConfirmDialog
        open={!!deletingBaseUrl}
        onOpenChange={(o) => !o && clearBaseUrlDelete()}
        title={`删除「${deletingBaseUrl?.name ?? ''}」?`}
        description="该服务会从项目服务清单中移除，所有模块里为它填写的地址也会一并失效；引用它的接口会回落到项目默认服务。"
        confirmText="删除"
        destructive
        onConfirm={confirmDeleteBaseUrl}
      />

      <ConfirmDialog
        open={!!deletingEnv}
        onOpenChange={(o) => !o && clearEnvDelete()}
        title={`删除环境「${deletingEnv?.name ?? ''}」?`}
        description="依赖它的接口会停止使用这个环境的前置 URL。"
        confirmText="删除"
        destructive
        onConfirm={confirmDeleteEnv}
      />
    </SettingsCategory>
  );
}
