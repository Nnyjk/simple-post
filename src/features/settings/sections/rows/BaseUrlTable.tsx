import { Globe, Star, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Tooltip } from '@/components/ui/tooltip';
import type { BaseUrlDefinition, Environment } from '@/types/domain';
import { HUE } from '@/lib/hue';
import { cn } from '@/lib/utils';
import type { EnvironmentSectionProps } from '../EnvironmentSection';
import { InlineNameCell } from './InlineNameCell';
import { KeyValueTable, RowTile } from './KeyValueTable';

/**
 * BaseUrlTable — list of named BaseUrl defs for the active env.
 *
 * The list chrome (empty state, row shell, "add" footer) is shared with
 * `VariableTable` via `KeyValueTable`; only the cells below are local.
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
  return (
    <KeyValueTable
      items={baseUrlDefinitions}
      keyOf={(def) => def.id}
      emptyText="项目还没有命名服务"
      addText="添加服务"
      onAdd={onAddBaseUrl}
      rowClassName="py-3"
      renderRow={(def) => {
        const url = env.baseUrls[def.id] ?? '';
        const isDefault = def.id === defaultBaseUrlDefinitionId;
        return (
          <>
            <RowTile concept="baseurl" icon={<Globe className="h-4 w-4" />} />

            {/* Label — inline-rename the service name */}
            <div className="min-w-0 w-44 shrink-0">
              <InlineNameCell
                value={def.name}
                placeholder="服务名"
                onCommit={(v) => updateBaseUrlDefinition(projectId, def.id, { name: v })}
                className="text-xs font-medium"
              />
              {isDefault && <div className="label-caps mt-0.5">项目默认</div>}
            </div>

            {/* URL input — the full-width editor */}
            <div className="min-w-0 flex-1">
              <Input
                value={url}
                onChange={(e) => setEnvironmentBaseUrl(env.id, def.id, e.target.value)}
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
                  onClick={() => onDeleteBaseUrl({ id: def.id, name: def.name })}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </Tooltip>
            </div>
          </>
        );
      }}
    />
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
            ? cn(HUE.module.border, HUE.module.bg, HUE.module.text)
            : 'border-border bg-transparent text-muted-foreground hover:bg-accent hover:text-foreground',
        )}
      >
        <Star className={cn('h-3.5 w-3.5', isDefault ? 'fill-current' : 'fill-transparent')} />
      </button>
    </Tooltip>
  );
}
