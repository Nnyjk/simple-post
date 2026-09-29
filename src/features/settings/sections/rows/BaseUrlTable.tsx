import { Globe, Plus, Star, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Tooltip } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import type { BaseUrlDefinition, Environment } from '@/types/domain';
import type { EnvironmentSectionProps } from '../EnvironmentSection';
import { InlineNameCell } from './InlineNameCell';

/**
 * BaseUrlTable — list of named BaseUrl defs for the active env.
 */
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

export function BaseUrlTable({
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
