import { useEffect, useMemo, useState } from 'react';
import {
  Plus,
  Trash2,
  Globe,
  Braces,
  Star,
  Pencil,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
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
  setEnvironmentVar: (envId: string, key: string, value: string) => void;
  setEnvironmentBaseUrl: (envId: string, defId: string, url: string) => void;
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
  setDefaultBaseUrlDefinition: (
    projectId: string,
    defId: string | null,
  ) => void;
}

const VAR_KEY_REGEX = /^[A-Za-z_][A-Za-z0-9_]*$/;

/**
 * Per-environment configuration, Apifox-style:
 *
 *   ┌── env tab bar ─────────────────────────────────────────┐
 *   │  本地环境   测试环境  正式环境  …  + 新建环境          │
 *   ├────────────────────────────────────────────────────────┤
 *   │  前置 URL                          + 添加服务           │
 *   │  🌐  默认      http://localhost:4000/demo  ★ 默认       │
 *   │  🌐  数据服务  http://127.0.0.1:9100/api                 │
 *   ├────────────────────────────────────────────────────────┤
 *   │  环境变量                          + 添加变量           │
 *   │  {{ }}  token  eyJhbGciOi...                            │
 *   │  {{ }}  env    dev                                      │
 *   └────────────────────────────────────────────────────────┘
 *
 * Per-row inline edit (rename service / variable, edit URL /
 * value, delete, mark-as-default) works the same as before — only
 * the outer shell changed (cards collapsed into a tab strip).
 */
export function EnvironmentSection({
  environments,
  projectId,
  setEnvironmentVar,
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
  // Browser-native `window.confirm` shows "localhost:5173 显示" as
  // the title and can't be repositioned — the dialogs we render here
  // go through `ConfirmDialog` for a consistent look. The "add baseUrl"
  // flow no longer needs a prompt because we use an inline empty row
  // (same UX as adding a variable).
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
    // Append a fresh env; the auto-select useEffect picks it up.
    addEnvironment(projectId, 'new-env');
  };

  const handleAddVar = () => {
    if (!activeEnv) return;
    // Inline empty-row append — same as the old card-based UX.
    setEnvironmentVar(activeEnv.id, '', '');
  };

  const handleAddBaseUrl = () => {
    if (!activeEnv) return;
    // Match the variable UX: append an empty row immediately. The user
    // can inline-rename the service name and fill the URL without any
    // intermediate modal — same flow as `handleAddVar`.
    const defId = addBaseUrlDefinition(projectId, '新服务');
    setEnvironmentBaseUrl(activeEnv.id, defId, '');
  };

  // ---- dialog confirm handlers ----
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
    <SettingsCategory hideDivider>
      <div className="space-y-3">
        {environments.length === 0 ? (
          <div className="rounded-md border border-dashed border-border bg-card/30 px-3 py-6 text-center text-xs text-muted-foreground">
            该项目还没有环境
          </div>
        ) : (
          <>
            {/* Tab strip — Apifox-style top-row navigation. */}
            <div
              role="tablist"
              aria-label="环境列表"
              className="flex flex-wrap items-center gap-1 border-b border-border pb-px"
            >
              {environments.map((env) => {
                const isActive = env.id === activeId;
                return (
                  <button
                    key={env.id}
                    type="button"
                    role="tab"
                    aria-selected={isActive}
                    onClick={() => setActiveId(env.id)}
                    className={cn(
                      'group relative flex h-8 items-center gap-1.5 rounded-t-md border-x border-t border-transparent px-3 text-xs transition-colors',
                      isActive
                        ? 'border-border bg-card text-foreground'
                        : 'text-muted-foreground hover:bg-accent/40 hover:text-foreground',
                    )}
                  >
                    <span className="font-mono font-medium">{env.name}</span>
                    {/* Active state is owned by the top-bar ENV dropdown
                        (the actual switcher). Showing it here would be
                        a duplicate affordance — by the time you're on
                        this settings page, the "active" decision is
                        already visible elsewhere. */}
                    {/* Active underline — pure decoration, drawn with
                        border-b so it lines up exactly with the strip
                        divider above. */}
                    {isActive && (
                      <span className="absolute inset-x-0 -bottom-px h-px bg-card" />
                    )}
                    {environments.length > 1 && (
                      <Tooltip content="删除环境" side="bottom">
                        <span
                          role="button"
                          aria-label={`删除环境 ${env.name}`}
                          tabIndex={-1}
                          onClick={(e) => {
                            e.stopPropagation();
                            setDeletingEnv({ id: env.id, name: env.name });
                          }}
                          className={cn(
                            'ml-1 inline-flex h-4 w-4 cursor-pointer items-center justify-center rounded text-muted-foreground/60 opacity-0 hover:bg-destructive/20 hover:text-destructive',
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
                variant="ghost"
                size="sm"
                className="ml-1 h-8 gap-1 text-xs"
                onClick={handleNewEnv}
              >
                <Plus className="h-3.5 w-3.5" />
                新建环境
              </Button>
            </div>

            {/* Active env body — tables. */}
            {activeEnv && (
              <div className="rounded-md border border-border bg-card/30">
                <EnvBody
                  env={activeEnv}
                  projectId={projectId}
                  setEnvironmentVar={setEnvironmentVar}
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

// ----------------------------------------------------------------------------
// EnvBody — one env's tables (URL rows + variable rows)
// ----------------------------------------------------------------------------

interface EnvBodyProps {
  env: Environment;
  projectId: string;

  setEnvironmentVar: EnvironmentSectionProps['setEnvironmentVar'];
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
    <div>
      <EnvHeader
        env={env}
        onRename={(name) => updateEnvironment(env.id, { name })}
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

      <VariableTable
        env={env}
        setEnvironmentVar={setEnvironmentVar}
        onAddVar={onAddVar}
      />
    </div>
  );
}

// ----------------------------------------------------------------------------
// EnvHeader — env name (inline rename) + activate-now toggle
// ----------------------------------------------------------------------------

function EnvHeader({
  env,
  onRename,
}: {
  env: Environment;
  onRename: (name: string) => void;
}) {
  // Activation happens through the ENV dropdown in the top bar — no
  // "active" badge / "设为活动" button needed in this panel. Keeping
  // the header to just the name + (optional) inline rename avoids
  // duplicating state and the "active" affordance is already on every
  // top-bar env picker row.
  return (
    <div className="flex items-center gap-2 border-b border-border/40 px-3 py-2">
      <InlineNameCell
        value={env.name}
        placeholder="环境名"
        onCommit={onRename}
        className="font-mono text-sm font-semibold"
      />
    </div>
  );
}

// ----------------------------------------------------------------------------
// BaseUrlTable
// ----------------------------------------------------------------------------

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
  /**
   * Trigger the parent confirm dialog with the def being deleted.
   * The parent owns the modal state.
   */
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
  return (
    <div>
      <SectionHeader
        icon={<Globe className="h-3 w-3" />}
        title="前置 URL"
        right={
          <Button
            variant="ghost"
            size="sm"
            className="h-6 gap-1 text-[10px] text-muted-foreground"
            onClick={onAddBaseUrl}
          >
            <Plus className="h-3 w-3" />
            添加服务
          </Button>
        }
      />
      {baseUrlDefinitions.length === 0 ? (
        <div className="px-3 pb-2 text-[11px] text-muted-foreground">
          项目还没有命名服务,点击「添加服务」新建一个
        </div>
      ) : (
        <table className="w-full text-[11px]">
          <colgroup>
            <col className="w-6" />
            <col className="w-32" />
            <col />
            <col className="w-24" />
            <col className="w-8" />
          </colgroup>
          <tbody>
            {baseUrlDefinitions.map((def) => {
              const url = env.baseUrls[def.id] ?? '';
              const isDefault = def.id === defaultBaseUrlDefinitionId;
              return (
                <tr
                  key={def.id}
                  className="border-t border-border/40 first:border-t-0"
                >
                  <td className="py-1 pl-3 pr-1 align-middle text-muted-foreground">
                    <Tooltip content="前置 URL" side="bottom">
                      <Globe className="h-3 w-3" />
                    </Tooltip>
                  </td>
                  <td className="py-1 pr-2 align-middle">
                    <InlineNameCell
                      value={def.name}
                      placeholder="服务名"
                      onCommit={(v) =>
                        updateBaseUrlDefinition(projectId, def.id, { name: v })
                      }
                    />
                  </td>
                  <td className="py-1 pr-2 align-middle">
                    <Input
                      value={url}
                      onChange={(e) =>
                        setEnvironmentBaseUrl(env.id, def.id, e.target.value)
                      }
                      placeholder="http:// 或 https:// 起始的 URL"
                      className="h-7 max-w-md font-mono text-[11px]"
                      spellCheck={false}
                    />
                  </td>
                  <td className="py-1 pr-2 align-middle">
                    <DefaultToggle
                      isDefault={isDefault}
                      onToggle={() =>
                        setDefaultBaseUrlDefinition(
                          projectId,
                          isDefault ? null : def.id,
                        )
                      }
                    />
                  </td>
                  <td className="py-1 pr-3 text-right align-middle">
                    <Tooltip content="删除服务" side="bottom">
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-6 w-6 text-muted-foreground hover:text-destructive"
                        onClick={() => onDeleteBaseUrl({ id: def.id, name: def.name })}
                      >
                        <Trash2 className="h-3 w-3" />
                      </Button>
                    </Tooltip>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </div>
  );
}

function DefaultToggle({
  isDefault,
  onToggle,
}: {
  isDefault: boolean;
  onToggle: () => void;
}) {
  return (
    <Tooltip content={isDefault ? '项目默认服务' : '设为项目默认'} side="top">
      <button
        type="button"
        onClick={onToggle}
        aria-pressed={isDefault}
        className={cn(
          'inline-flex h-6 items-center gap-1 rounded px-1.5 text-[10px] transition-colors',
          isDefault
            ? 'bg-primary/15 text-primary'
            : 'text-muted-foreground hover:bg-accent hover:text-foreground',
        )}
      >
        <Star
          className={cn('h-3 w-3', isDefault ? 'fill-current' : 'fill-transparent')}
        />
        <span>{isDefault ? '默认' : '设为默认'}</span>
      </button>
    </Tooltip>
  );
}

// ----------------------------------------------------------------------------
// VariableTable
// ----------------------------------------------------------------------------

interface VariableTableProps {
  env: Environment;
  setEnvironmentVar: EnvironmentSectionProps['setEnvironmentVar'];
  onAddVar: () => void;
}

function VariableTable({ env, setEnvironmentVar, onAddVar }: VariableTableProps) {
  const entries = Object.entries(env.variables);

  return (
    <div className="border-t border-border/40">
      <SectionHeader
        icon={<Braces className="h-3 w-3" />}
        title="环境变量"
        right={
          <Button
            variant="ghost"
            size="sm"
            className="h-6 gap-1 text-[10px] text-muted-foreground"
            onClick={onAddVar}
          >
            <Plus className="h-3 w-3" />
            添加变量
          </Button>
        }
      />
      {entries.length === 0 ? (
        <div className="px-3 pb-2 text-[11px] text-muted-foreground">
          暂无变量
        </div>
      ) : (
        <table className="w-full text-[11px]">
          <colgroup>
            <col className="w-6" />
            <col className="w-32" />
            <col />
            <col className="w-8" />
          </colgroup>
          <tbody>
            {entries.map(([k, v]) => (
              <tr key={k} className="border-t border-border/40 first:border-t-0">
                <td className="py-1 pl-3 pr-1 align-middle text-muted-foreground">
                  <Tooltip content="环境变量" side="bottom">
                    <Braces className="h-3 w-3" />
                  </Tooltip>
                </td>
                <td className="py-1 pr-2 align-middle">
                  <InlineNameCell
                    value={k}
                    placeholder="变量名"
                    validate={(v) =>
                      VAR_KEY_REGEX.test(v.trim())
                        ? null
                        : '变量名仅允许字母数字下划线,且不能以数字开头'
                    }
                    onCommit={(newKey) => {
                      if (!newKey || newKey === k) return;
                      // Rename: drop the old key, set the new one. We use
                      // setEnvironmentVar for both so the validation goes
                      // through the same code path.
                      setEnvironmentVar(env.id, k, '');
                      setEnvironmentVar(env.id, newKey, v);
                    }}
                  />
                </td>
                <td className="py-1 pr-2 align-middle">
                  <Input
                    value={v}
                    onChange={(e) =>
                      setEnvironmentVar(env.id, k, e.target.value)
                    }
                    placeholder="值(支持 {{其它变量}} 引用)"
                    className="h-7 max-w-md font-mono text-[11px]"
                    spellCheck={false}
                  />
                </td>
                <td className="py-1 pr-3 text-right align-middle">
                  <Tooltip content="删除变量" side="bottom">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6 text-muted-foreground hover:text-destructive"
                      onClick={() => {
                        // "Deleting" an empty-key placeholder just
                        // re-clears it (no-op). For real keys, blanking
                        // it removes it from the dict.
                        setEnvironmentVar(env.id, k, '');
                      }}
                    >
                      <Trash2 className="h-3 w-3" />
                    </Button>
                  </Tooltip>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

// ----------------------------------------------------------------------------
// Shared bits
// ----------------------------------------------------------------------------

function SectionHeader({
  icon,
  title,
  right,
}: {
  icon?: React.ReactNode;
  title: string;
  right?: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-2 px-3 py-1.5">
      {icon && <span className="text-muted-foreground">{icon}</span>}
      <span className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
        {title}
      </span>
      <div className="ml-auto">{right}</div>
    </div>
  );
}

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
          'flex h-7 w-full items-center gap-1 rounded-md px-2 text-left text-[11px] transition-colors',
          'hover:bg-accent/40',
          !value && 'text-muted-foreground/60',
          className,
        )}
      >
        {value || placeholder}
        <Pencil className="h-2.5 w-2.5 opacity-0 transition-opacity group-hover:opacity-100" />
      </button>
    );
  }

  return (
    <div className="flex flex-col">
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
        placeholder={placeholder}
        className="h-7 max-w-md font-mono text-[11px]"
      />
      {error && (
        <span className="mt-0.5 text-[10px] text-destructive">{error}</span>
      )}
    </div>
  );
}