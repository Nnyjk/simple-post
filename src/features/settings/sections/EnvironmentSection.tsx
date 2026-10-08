import { useCallback, useEffect, useMemo, useState } from 'react';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { EmptyHint } from '@/components/ui/empty-hint';
import { SettingsCategory } from '../SettingsCategory';
import { useEnvDialogs } from './hooks/useEnvDialogs';
import { EnvBody } from './rows/EnvBody';
import { EnvSwitcher } from './rows/EnvSwitcher';
import type { BaseUrlDefinition, Environment } from '@/types/domain';

export interface EnvironmentSectionProps {
  // ---- scope ----
  environments: Environment[];
  projectId: string;

  // ---- env-scoped actions ----
  setEnvironmentVar: (envId: string, key: string, value: string | null) => void;
  renameEnvironmentVar: (envId: string, fromKey: string, toKey: string) => void;
  setEnvironmentBaseUrl: (envId: string, defId: string, url: string | null) => void;
  /** Resolves the new env's id, or `null` when the create failed. */
  addEnvironment: (projectId: string, name: string) => Promise<string | null>;
  deleteEnvironment: (envId: string) => void;
  updateEnvironment: (envId: string, patch: Partial<Environment>) => void;

  // ---- project-scoped baseUrl definitions ----
  baseUrlDefinitions: readonly BaseUrlDefinition[];
  defaultBaseUrlDefinitionId: string | null;
  addBaseUrlDefinition: (projectId: string, name: string) => string;
  updateBaseUrlDefinition: (
    projectId: string,
    defId: string,
    patch: Partial<Pick<BaseUrlDefinition, 'name' | 'description'>>,
  ) => void;
  deleteBaseUrlDefinition: (projectId: string, defId: string) => void;
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
 *   ├──────────── 前置 URL · BaseUrl 定义 ────────────────────┤
 *   │  🌐  默认      http://localhost:4000/demo  [默认] [×]   │
 *   │  🌐  数据服务  http://127.0.0.1:9100/api        [×]    │
 *   │                  + 添加服务                            │
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
 * Per-row inline edit (rename service/variable, edit URL/value,
 * delete, mark-as-default) works as before; only the outer shell
 * changed.
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
  setEnvironmentVar,
  renameEnvironmentVar,
  setEnvironmentBaseUrl,
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

  const handleAddBaseUrl = () => {
    if (!activeEnv) return;
    // Match the variable UX: append an empty row immediately.
    const defId = addBaseUrlDefinition(projectId, '新服务');
    setEnvironmentBaseUrl(activeEnv.id, defId, '');
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
                setEnvironmentVar={setEnvironmentVar}
                renameEnvironmentVar={renameEnvironmentVar}
                setEnvironmentBaseUrl={setEnvironmentBaseUrl}
                updateEnvironment={updateEnvironment}
                onAddVar={handleAddVar}
                onAddBaseUrl={handleAddBaseUrl}
                baseUrlDefinitions={baseUrlDefinitions}
                defaultBaseUrlDefinitionId={defaultBaseUrlDefinitionId}
                updateBaseUrlDefinition={updateBaseUrlDefinition}
                deleteBaseUrlDefinition={deleteBaseUrlDefinition}
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
        description="该操作会从所有环境中移除对应的 URL。"
        confirmText="删除"
        destructive
        onConfirm={confirmDeleteBaseUrl}
      />

      <ConfirmDialog
        open={!!deletingEnv}
        onOpenChange={(o) => !o && clearEnvDelete()}
        title={`删除环境「${deletingEnv?.name ?? ''}」?`}
        description="依赖它的接口会回落到项目默认 baseUrl。"
        confirmText="删除"
        destructive
        onConfirm={confirmDeleteEnv}
      />
    </SettingsCategory>
  );
}
