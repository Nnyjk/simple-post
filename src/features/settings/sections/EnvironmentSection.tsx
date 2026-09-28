import { useEffect, useMemo, useState } from 'react';
import { Plus, Trash2, Globe, Braces, Star, Pencil, Cloud, Rocket, TestTube } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Tooltip } from '@/components/ui/tooltip';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { cn } from '@/lib/utils';
import { SettingsCategory } from '../SettingsCategory';
import type { BaseUrlDefinition, Environment } from '@/types/domain';

export interface EnvironmentSectionProps {
  // ---- scope ----
  environments: Environment[];
  projectId: string;

  // ---- env-scoped actions ----
  setEnvironmentVar: (envId: string, key: string, value: string | null) => void;
  renameEnvironmentVar: (envId: string, fromKey: string, toKey: string) => void;
  setEnvironmentBaseUrl: (envId: string, defId: string, url: string | null) => void;
  addEnvironment: (projectId: string, name: string) => void;
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

const VAR_KEY_REGEX = /^[A-Za-z_][A-Za-z0-9_]*$/;

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

  // ---- modal state ----
  const [deletingBaseUrl, setDeletingBaseUrl] = useState<{
    id: string;
    name: string;
  } | null>(null);
  const [deletingEnv, setDeletingEnv] = useState<{
    id: string;
    name: string;
  } | null>(null);

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

  const handleNewEnv = () => {
    addEnvironment(projectId, 'new-env');
  };

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

  const confirmDeleteBaseUrl = () => {
    if (deletingBaseUrl) {
      deleteBaseUrlDefinition(projectId, deletingBaseUrl.id);
    }
  };

  const confirmDeleteEnv = () => {
    if (deletingEnv) {
      deleteEnvironment(deletingEnv.id);
    }
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
        {environments.length === 0 ? (
          <div className="rounded-lg border border-dashed border-border bg-card/30 px-3 py-8 text-center text-xs text-muted-foreground">
            该项目还没有环境
          </div>
        ) : (
          <>
            {/* ----- Env switcher (top tab strip) ----------------- */}
            <EnvSwitcher
              environments={environments}
              activeId={activeId}
              onPick={setActiveId}
              onRequestDelete={(env) => setDeletingEnv({ id: env.id, name: env.name })}
              onAdd={handleNewEnv}
            />

            {/* ----- Active env body ------------------------------ */}
            {activeEnv && (
              <div className="overflow-hidden rounded-lg border border-border/70 bg-card/40 shadow-[0_1px_0_0_rgba(255,255,255,0.02)_inset]">
                <EnvBody
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
                  onDeleteBaseUrl={(def) => setDeletingBaseUrl(def)}
                />
              </div>
            )}
          </>
        )}
      </div>

      {/* ---------- Dialogs ---------- */}

      <ConfirmDialog
        open={!!deletingBaseUrl}
        onOpenChange={(o) => !o && setDeletingBaseUrl(null)}
        title={`删除「${deletingBaseUrl?.name ?? ''}」?`}
        description="该操作会从所有环境中移除对应的 URL。"
        confirmText="删除"
        destructive
        onConfirm={confirmDeleteBaseUrl}
      />

      <ConfirmDialog
        open={!!deletingEnv}
        onOpenChange={(o) => !o && setDeletingEnv(null)}
        title={`删除环境「${deletingEnv?.name ?? ''}」?`}
        description="依赖它的接口会回落到项目默认 baseUrl。"
        confirmText="删除"
        destructive
        onConfirm={confirmDeleteEnv}
      />
    </SettingsCategory>
  );
}

// ============================================================================
// EnvSwitcher — top-of-section env picker
// ============================================================================

function EnvSwitcher({
  environments,
  activeId,
  onPick,
  onRequestDelete,
  onAdd,
}: {
  environments: Environment[];
  activeId: string | null;
  onPick: (id: string) => void;
  onRequestDelete: (env: Environment) => void;
  onAdd: () => void;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      {environments.map((env) => {
        const isActive = env.id === activeId;
        return (
          <button
            key={env.id}
            type="button"
            role="tab"
            aria-selected={isActive}
            onClick={() => onPick(env.id)}
            className={cn(
              'group relative flex items-center gap-2 rounded-lg border px-2.5 py-1.5 text-xs transition-colors',
              // Coloured icon container matches the project's tier
              // language (sky = base / data, amber = config / builds,
              // violet = production-ish). Picked automatically from the
              // env name so users don't have to tag each one.
              isActive
                ? 'border-primary/40 bg-primary/10 text-foreground shadow-[inset_0_0_0_1px_hsl(var(--primary)/0.25)]'
                : 'border-border bg-card/30 text-muted-foreground hover:bg-accent/40 hover:text-foreground',
            )}
          >
            <span
              aria-hidden
              className={cn(
                'inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md border',
                isActive
                  ? 'border-primary/40 bg-primary/15 text-primary'
                  : 'border-border bg-muted/60 text-muted-foreground',
              )}
            >
              {envIcon(env.name)}
            </span>
            <span className="font-mono font-medium">{env.name}</span>
            {environments.length > 1 && (
              <Tooltip content="删除环境" side="bottom">
                <span
                  role="button"
                  aria-label={`删除环境 ${env.name}`}
                  tabIndex={-1}
                  onClick={(e) => {
                    e.stopPropagation();
                    onRequestDelete(env);
                  }}
                  className={cn(
                    'ml-0.5 inline-flex h-4 w-4 cursor-pointer items-center justify-center rounded text-muted-foreground/60 opacity-0 hover:bg-destructive/20 hover:text-destructive',
                    'group-hover:opacity-100',
                    isActive && 'opacity-100',
                  )}
                >
                  <Trash2 className="h-3 w-3" />
                </span>
              </Tooltip>
            )}
          </button>
        );
      })}
      <Button
        variant="outline"
        size="sm"
        className="h-8 gap-1 border-dashed text-xs text-muted-foreground hover:text-foreground"
        onClick={onAdd}
      >
        <Plus className="h-3.5 w-3.5" />
        新建环境
      </Button>
    </div>
  );
}

/**
 * Pick an icon for the env tab based on its name. The mapping is loose
 * by design — names that don't match anything get the neutral Cloud
 * icon, which still differentiates the env visually without forcing the
 * user to pick an icon manually.
 */
function envIcon(name: string) {
  const n = name.toLowerCase();
  if (n.includes('prod') || n.includes('release') || n.includes('正式') || n.includes('线上')) {
    return <Rocket className="h-3.5 w-3.5" />;
  }
  if (n.includes('stag') || n.includes('pre') || n.includes('测试') || n.includes('预发')) {
    return <TestTube className="h-3.5 w-3.5" />;
  }
  if (n.includes('dev') || n.includes('local') || n.includes('开发') || n.includes('本地')) {
    return <TestTube className="h-3.5 w-3.5" />;
  }
  return <Cloud className="h-3.5 w-3.5" />;
}

// ============================================================================
// EnvBody — one env's tables (URL rows + variable rows)
// ============================================================================

interface EnvBodyProps {
  env: Environment;
  projectId: string;

  setEnvironmentVar: EnvironmentSectionProps['setEnvironmentVar'];
  renameEnvironmentVar: EnvironmentSectionProps['renameEnvironmentVar'];
  setEnvironmentBaseUrl: EnvironmentSectionProps['setEnvironmentBaseUrl'];
  updateEnvironment: EnvironmentSectionProps['updateEnvironment'];
  onAddVar: () => void;
  onAddBaseUrl: () => void;

  baseUrlDefinitions: EnvironmentSectionProps['baseUrlDefinitions'];
  defaultBaseUrlDefinitionId: EnvironmentSectionProps['defaultBaseUrlDefinitionId'];
  updateBaseUrlDefinition: EnvironmentSectionProps['updateBaseUrlDefinition'];
  deleteBaseUrlDefinition: EnvironmentSectionProps['deleteBaseUrlDefinition'];
  setDefaultBaseUrlDefinition: EnvironmentSectionProps['setDefaultBaseUrlDefinition'];
  onDeleteBaseUrl: (def: { id: string; name: string }) => void;
}

function EnvBody({
  env,
  projectId,
  setEnvironmentVar,
  renameEnvironmentVar,
  setEnvironmentBaseUrl,
  updateEnvironment,
  onAddVar,
  onAddBaseUrl,
  baseUrlDefinitions,
  defaultBaseUrlDefinitionId,
  updateBaseUrlDefinition,
  deleteBaseUrlDefinition,
  setDefaultBaseUrlDefinition,
  onDeleteBaseUrl,
}: EnvBodyProps) {
  return (
    <div className="flex flex-col">
      <EnvHeader env={env} onRename={(name) => updateEnvironment(env.id, { name })} />

      <GroupHeader
        icon={<Globe className="h-3.5 w-3.5" />}
        title="前置 URL"
        description="项目级 BaseUrl 定义 + 当前环境的覆盖值"
        meta={baseUrlDefinitions.length > 0 ? `${baseUrlDefinitions.length} 个服务` : undefined}
      />
      <BaseUrlTable
        env={env}
        projectId={projectId}
        baseUrlDefinitions={baseUrlDefinitions}
        defaultBaseUrlDefinitionId={defaultBaseUrlDefinitionId}
        setEnvironmentBaseUrl={setEnvironmentBaseUrl}
        updateBaseUrlDefinition={updateBaseUrlDefinition}
        deleteBaseUrlDefinition={deleteBaseUrlDefinition}
        setDefaultBaseUrlDefinition={setDefaultBaseUrlDefinition}
        onAddBaseUrl={onAddBaseUrl}
        onDeleteBaseUrl={onDeleteBaseUrl}
      />

      <GroupHeader
        icon={<Braces className="h-3.5 w-3.5" />}
        title="环境变量"
        description="在 URL/Headers/Body 中通过 {{key}} 引用"
        meta={
          Object.keys(env.variables).length > 0
            ? `${Object.keys(env.variables).length} 个变量`
            : undefined
        }
      />
      <VariableTable
        env={env}
        setEnvironmentVar={setEnvironmentVar}
        renameEnvironmentVar={renameEnvironmentVar}
        onAddVar={onAddVar}
      />
    </div>
  );
}

// ============================================================================
// EnvHeader — env name (inline rename)
// ============================================================================

function EnvHeader({ env, onRename }: { env: Environment; onRename: (name: string) => void }) {
  return (
    <div className="flex items-center gap-2 border-b border-border/60 px-4 py-2.5">
      <span
        aria-hidden
        className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-primary/30 bg-primary/15 text-primary"
      >
        {envIcon(env.name)}
      </span>
      <div className="min-w-0">
        <InlineNameCell
          value={env.name}
          placeholder="环境名"
          onCommit={onRename}
          className="text-sm font-semibold"
        />
      </div>
    </div>
  );
}

// ============================================================================
// GroupHeader — section sub-title inside the env body
// ============================================================================

function GroupHeader({
  icon,
  title,
  description,
  meta,
}: {
  icon?: React.ReactNode;
  title: string;
  description?: string;
  meta?: string;
}) {
  return (
    <div className="flex items-start gap-2 border-b border-border/60 bg-muted/20 px-4 py-2">
      {icon && (
        <span
          aria-hidden
          className="mt-0.5 inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-background text-muted-foreground"
        >
          {icon}
        </span>
      )}
      <div className="min-w-0">
        <div className="text-[10px] font-semibold uppercase tracking-wider text-foreground">
          {title}
        </div>
        {description && (
          <div className="mt-0.5 text-[10px] text-muted-foreground">
            {description}
            {meta ? (
              <>
                {' · '}
                <span className="font-mono">{meta}</span>
              </>
            ) : null}
          </div>
        )}
      </div>
    </div>
  );
}

// ============================================================================
// BaseUrlTable — list of named BaseUrl defs for the active env
// ============================================================================

interface BaseUrlTableProps {
  env: Environment;
  projectId: string;
  baseUrlDefinitions: readonly BaseUrlDefinition[];
  defaultBaseUrlDefinitionId: string | null;
  setEnvironmentBaseUrl: EnvironmentSectionProps['setEnvironmentBaseUrl'];
  updateBaseUrlDefinition: EnvironmentSectionProps['updateBaseUrlDefinition'];
  deleteBaseUrlDefinition: EnvironmentSectionProps['deleteBaseUrlDefinition'];
  setDefaultBaseUrlDefinition: EnvironmentSectionProps['setDefaultBaseUrlDefinition'];
  onAddBaseUrl: () => void;
  onDeleteBaseUrl: (def: { id: string; name: string }) => void;
}

function BaseUrlTable({
  env,
  projectId,
  baseUrlDefinitions,
  defaultBaseUrlDefinitionId,
  setEnvironmentBaseUrl,
  updateBaseUrlDefinition,
  setDefaultBaseUrlDefinition,
  onAddBaseUrl,
  onDeleteBaseUrl,
}: BaseUrlTableProps) {
  if (baseUrlDefinitions.length === 0) {
    return (
      <div className="flex flex-col items-center gap-2 px-4 py-6 text-center">
        <div className="text-[11px] text-muted-foreground">项目还没有命名服务</div>
        <Button variant="default" size="sm" className="h-7 gap-1 text-xs" onClick={onAddBaseUrl}>
          <Plus className="h-3.5 w-3.5" />
          添加服务
        </Button>
      </div>
    );
  }

  return (
    <div className="flex flex-col">
      {baseUrlDefinitions.map((def, idx) => {
        const url = env.baseUrls[def.id] ?? '';
        const isDefault = def.id === defaultBaseUrlDefinitionId;
        return (
          <BaseUrlRow
            key={def.id}
            flush={idx === 0}
            def={def}
            envId={env.id}
            url={url}
            isDefault={isDefault}
            projectId={projectId}
            setEnvironmentBaseUrl={setEnvironmentBaseUrl}
            updateBaseUrlDefinition={updateBaseUrlDefinition}
            setDefaultBaseUrlDefinition={setDefaultBaseUrlDefinition}
            onDelete={onDeleteBaseUrl}
          />
        );
      })}
      <button
        type="button"
        onClick={onAddBaseUrl}
        className="flex items-center gap-2 border-t border-border/60 px-4 py-2 text-[11px] text-muted-foreground transition-colors hover:bg-accent/40 hover:text-foreground"
      >
        <Plus className="h-3.5 w-3.5" />
        添加服务
      </button>
    </div>
  );
}

interface BaseUrlRowProps {
  flush: boolean;
  def: BaseUrlDefinition;
  envId: string;
  url: string;
  isDefault: boolean;
  projectId: string;
  setEnvironmentBaseUrl: BaseUrlTableProps['setEnvironmentBaseUrl'];
  updateBaseUrlDefinition: BaseUrlTableProps['updateBaseUrlDefinition'];
  setDefaultBaseUrlDefinition: BaseUrlTableProps['setDefaultBaseUrlDefinition'];
  onDelete: (def: { id: string; name: string }) => void;
}

function BaseUrlRow({
  flush,
  def,
  url,
  isDefault,
  envId,
  projectId,
  setEnvironmentBaseUrl,
  updateBaseUrlDefinition,
  setDefaultBaseUrlDefinition,
  onDelete,
}: BaseUrlRowProps) {
  return (
    <div
      className={cn(
        'group flex items-start gap-3 px-4 py-3',
        !flush && 'border-t border-border/60',
      )}
    >
      {/* Icon container — sky tones for base URLs */}
      <span
        aria-hidden
        className="mt-0.5 inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-sky-500/30 bg-sky-500/15 text-sky-500"
      >
        <Globe className="h-4 w-4" />
      </span>

      {/* Label — inline-rename the service name */}
      <div className="min-w-0 w-44 shrink-0">
        <InlineNameCell
          value={def.name}
          placeholder="服务名"
          onCommit={(v) => updateBaseUrlDefinition(projectId, def.id, { name: v })}
          className="text-xs font-medium"
        />
        {isDefault && (
          <div className="mt-0.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
            项目默认
          </div>
        )}
      </div>

      {/* URL input — the full-width editor */}
      <div className="min-w-0 flex-1">
        <Input
          value={url}
          onChange={(e) => setEnvironmentBaseUrl(envId, def.id, e.target.value)}
          placeholder="http:// 或 https:// 起始的 URL"
          className="h-8 font-mono text-xs"
          spellCheck={false}
        />
      </div>

      {/* Trailing actions */}
      <div className="flex shrink-0 items-center gap-1">
        <DefaultToggle
          isDefault={isDefault}
          onToggle={() => setDefaultBaseUrlDefinition(projectId, isDefault ? null : def.id)}
        />
        <Tooltip content="删除服务" side="bottom">
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 text-muted-foreground opacity-0 hover:text-destructive group-hover:opacity-100"
            onClick={() => onDelete({ id: def.id, name: def.name })}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </Tooltip>
      </div>
    </div>
  );
}

function DefaultToggle({ isDefault, onToggle }: { isDefault: boolean; onToggle: () => void }) {
  return (
    <Tooltip content={isDefault ? '项目默认服务' : '设为项目默认'} side="top">
      <button
        type="button"
        onClick={onToggle}
        aria-pressed={isDefault}
        className={cn(
          'inline-flex h-7 w-7 items-center justify-center rounded-md border transition-colors',
          isDefault
            ? 'border-amber-500/40 bg-amber-500/15 text-amber-500'
            : 'border-border bg-transparent text-muted-foreground hover:bg-accent hover:text-foreground',
        )}
      >
        <Star className={cn('h-3.5 w-3.5', isDefault ? 'fill-current' : 'fill-transparent')} />
      </button>
    </Tooltip>
  );
}

// ============================================================================
// VariableTable
// ============================================================================

interface VariableTableProps {
  env: Environment;
  setEnvironmentVar: EnvironmentSectionProps['setEnvironmentVar'];
  renameEnvironmentVar: EnvironmentSectionProps['renameEnvironmentVar'];
  onAddVar: () => void;
}

function VariableTable({
  env,
  setEnvironmentVar,
  renameEnvironmentVar,
  onAddVar,
}: VariableTableProps) {
  const entries = Object.entries(env.variables);

  if (entries.length === 0) {
    return (
      <div className="flex flex-col items-center gap-2 px-4 py-6 text-center">
        <div className="text-[11px] text-muted-foreground">暂无变量</div>
        <Button variant="default" size="sm" className="h-7 gap-1 text-xs" onClick={onAddVar}>
          <Plus className="h-3.5 w-3.5" />
          添加变量
        </Button>
      </div>
    );
  }

  return (
    <div className="flex flex-col">
      {entries.map(([k, v], idx) => (
        <VariableRow
          key={k || `__empty_${idx}`}
          flush={idx === 0}
          name={k}
          value={v}
          envId={env.id}
          setEnvironmentVar={setEnvironmentVar}
          renameEnvironmentVar={renameEnvironmentVar}
        />
      ))}
      <button
        type="button"
        onClick={onAddVar}
        className="flex items-center gap-2 border-t border-border/60 px-4 py-2 text-[11px] text-muted-foreground transition-colors hover:bg-accent/40 hover:text-foreground"
      >
        <Plus className="h-3.5 w-3.5" />
        添加变量
      </button>
    </div>
  );
}

interface VariableRowProps {
  flush: boolean;
  name: string;
  value: string;
  envId: string;
  setEnvironmentVar: VariableTableProps['setEnvironmentVar'];
  renameEnvironmentVar: VariableTableProps['renameEnvironmentVar'];
}

function VariableRow({
  flush,
  name,
  value,
  envId,
  setEnvironmentVar,
  renameEnvironmentVar,
}: VariableRowProps) {
  return (
    <div
      className={cn(
        'group flex items-start gap-3 px-4 py-2.5',
        !flush && 'border-t border-border/60',
      )}
    >
      {/* Icon container — amber for "config"-shaped rows */}
      <span
        aria-hidden
        className="mt-0.5 inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-amber-500/30 bg-amber-500/15 text-amber-500"
      >
        <Braces className="h-4 w-4" />
      </span>

      {/* Variable key — inline rename */}
      <div className="min-w-0 w-44 shrink-0">
        <InlineNameCell
          value={name}
          placeholder="变量名"
          validate={(v) =>
            VAR_KEY_REGEX.test(v.trim()) ? null : '变量名仅允许字母数字下划线,且不能以数字开头'
          }
          onCommit={(newKey) => {
            if (!newKey || newKey === name) return;
            // 一个命令完成重命名。之前这里是「先删旧 key，再写新 key」
            // 两次 setEnvironmentVar，两次事务之间变量是消失的——进程
            // 中断或第二次调用失败就会把变量删掉。改成单次原子调用后
            // 不存在这个窗口。
            renameEnvironmentVar(envId, name, newKey);
          }}
          className="font-mono text-xs font-medium"
        />
        <div className="mt-0.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
          {`{{${name || '...'}}}`}
        </div>
      </div>

      {/* Variable value */}
      <div className="min-w-0 flex-1">
        <Input
          value={value}
          onChange={(e) => setEnvironmentVar(envId, name, e.target.value)}
          placeholder="值(支持 {{其它变量}} 引用)"
          className="h-8 font-mono text-xs"
          spellCheck={false}
        />
      </div>

      {/* Trailing actions */}
      <div className="flex shrink-0 items-center gap-1">
        <Tooltip content="删除变量" side="bottom">
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 text-muted-foreground opacity-0 hover:text-destructive group-hover:opacity-100"
            onClick={() => setEnvironmentVar(envId, name, null)}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </Tooltip>
      </div>
    </div>
  );
}

// ============================================================================
// Shared bits
// ============================================================================

/**
 * A compact click-to-edit text cell. Click the value to swap in an
 * input; blur or Enter commits, Escape reverts. Optional `validate`
 * rejects commits with an inline error message.
 */
function InlineNameCell({
  value,
  placeholder,
  validate,
  onCommit,
  className,
}: {
  value: string;
  placeholder: string;
  validate?: (next: string) => string | null;
  onCommit: (next: string) => void;
  className?: string;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const [error, setError] = useState<string | null>(null);

  const begin = () => {
    setDraft(value);
    setError(null);
    setEditing(true);
  };
  const cancel = () => {
    setEditing(false);
    setError(null);
  };
  const commit = () => {
    const trimmed = draft.trim();
    if (validate) {
      const msg = validate(trimmed);
      if (msg) {
        setError(msg);
        return;
      }
    }
    if (trimmed !== value) onCommit(trimmed);
    setEditing(false);
    setError(null);
  };

  if (!editing) {
    return (
      <button
        type="button"
        onClick={begin}
        className={cn(
          'group/cell flex w-full items-center gap-1 rounded px-1.5 py-0.5 text-left transition-colors hover:bg-accent/40',
          !value && 'text-muted-foreground/60 italic',
          className,
        )}
      >
        <span className="min-w-0 truncate">{value || placeholder}</span>
        <Pencil className="h-3 w-3 shrink-0 opacity-0 transition-opacity group-hover/cell:opacity-100" />
      </button>
    );
  }

  return (
    <div className="flex flex-col gap-1">
      <Input
        autoFocus
        value={draft}
        onChange={(e) => {
          setDraft(e.target.value);
          if (error) setError(null);
        }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            commit();
          } else if (e.key === 'Escape') {
            e.preventDefault();
            cancel();
          }
        }}
        className="h-7 font-mono text-xs"
      />
      {error && <div className="px-1 text-[10px] text-destructive">{error}</div>}
    </div>
  );
}
