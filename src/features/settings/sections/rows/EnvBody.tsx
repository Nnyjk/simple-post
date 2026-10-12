import { Braces, Globe } from 'lucide-react';
import type { Environment, ResourceNode } from '@/types/domain';
import type { EnvironmentSectionProps } from '../EnvironmentSection';
import { EnvHeader } from './EnvHeader';
import { GroupHeader } from './GroupHeader';
import { ModuleBaseUrlTable } from './ModuleBaseUrlTable';
import { VariableTable } from './VariableTable';

/**
 * EnvBody — one env's tables (前置 URL rows + variable rows).
 */
interface EnvBodyProps {
  env: Environment;
  projectId: string;
  /** This project's modules — the grouping axis of the 前置 URL table. */
  modules: readonly ResourceNode[];

  setEnvironmentVar: EnvironmentSectionProps['setEnvironmentVar'];
  renameEnvironmentVar: EnvironmentSectionProps['renameEnvironmentVar'];
  setEnvironmentModuleBaseUrl: EnvironmentSectionProps['setEnvironmentModuleBaseUrl'];
  updateEnvironment: EnvironmentSectionProps['updateEnvironment'];
  onAddVar: () => void;

  baseUrlDefinitions: EnvironmentSectionProps['baseUrlDefinitions'];
  defaultBaseUrlDefinitionId: string | null;
  addBaseUrlDefinition: EnvironmentSectionProps['addBaseUrlDefinition'];
  updateBaseUrlDefinition: EnvironmentSectionProps['updateBaseUrlDefinition'];
  setDefaultBaseUrlDefinition: EnvironmentSectionProps['setDefaultBaseUrlDefinition'];
  onDeleteBaseUrl: (def: { id: string; name: string }) => void;
  /** Open the env name already focused — set for a just-created env. */
  autoRename?: boolean;
  /** The auto-open was consumed; the shell clears its pending-rename flag. */
  onRenameEnd?: () => void;
}

export function EnvBody({
  env,
  projectId,
  modules,
  setEnvironmentVar,
  renameEnvironmentVar,
  setEnvironmentModuleBaseUrl,
  updateEnvironment,
  onAddVar,
  baseUrlDefinitions,
  defaultBaseUrlDefinitionId,
  addBaseUrlDefinition,
  updateBaseUrlDefinition,
  setDefaultBaseUrlDefinition,
  onDeleteBaseUrl,
  autoRename,
  onRenameEnd,
}: EnvBodyProps) {
  return (
    <div className="flex flex-col">
      <EnvHeader
        env={env}
        onRename={(name) => updateEnvironment(env.id, { name })}
        autoRename={autoRename}
        onRenameEnd={onRenameEnd}
      />

      <GroupHeader
        icon={<Globe className="h-3.5 w-3.5" />}
        title="前置 URL"
        // Describes the model as it is: the service list belongs to the
        // project and every module sees all of it, while the address in
        // the third column is this module's own value for that service in
        // this environment.
        description="项目共用一份服务清单，每个模块各填各的前置 URL"
        meta={baseUrlDefinitions.length > 0 ? `${baseUrlDefinitions.length} 个服务` : undefined}
      />
      <ModuleBaseUrlTable
        env={env}
        projectId={projectId}
        modules={modules}
        baseUrlDefinitions={baseUrlDefinitions}
        defaultBaseUrlDefinitionId={defaultBaseUrlDefinitionId}
        setEnvironmentModuleBaseUrl={setEnvironmentModuleBaseUrl}
        addBaseUrlDefinition={addBaseUrlDefinition}
        updateBaseUrlDefinition={updateBaseUrlDefinition}
        setDefaultBaseUrlDefinition={setDefaultBaseUrlDefinition}
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
