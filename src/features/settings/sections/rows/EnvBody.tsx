import { Braces, Globe } from 'lucide-react';
import type { Environment, ResourceNode } from '@/types/domain';
import type { EnvironmentSectionProps } from '../EnvironmentSection';
import { GroupHeader } from './GroupHeader';
import { ModuleBaseUrlTable } from './ModuleBaseUrlTable';
import { VariableTable } from './VariableTable';

/**
 * EnvBody — one env's tables (前置 URL rows + variable rows).
 *
 * No name row. The switcher above this already says which environment is
 * open and owns the rename; repeating it here is what made the project /
 * settings env view look different from the module one, which has never
 * had the row. Keyed by env id, so it remounts per environment.
 */
interface EnvBodyProps {
  env: Environment;
  projectId: string;
  /** This project's modules — the grouping axis of the 前置 URL table. */
  modules: readonly ResourceNode[];

  setEnvironmentVar: EnvironmentSectionProps['setEnvironmentVar'];
  renameEnvironmentVar: EnvironmentSectionProps['renameEnvironmentVar'];
  setEnvironmentModuleBaseUrl: EnvironmentSectionProps['setEnvironmentModuleBaseUrl'];
  onAddVar: () => void;

  baseUrlDefinitions: EnvironmentSectionProps['baseUrlDefinitions'];
  defaultBaseUrlDefinitionId: string | null;
  addBaseUrlDefinition: EnvironmentSectionProps['addBaseUrlDefinition'];
  updateBaseUrlDefinition: EnvironmentSectionProps['updateBaseUrlDefinition'];
  setDefaultBaseUrlDefinition: EnvironmentSectionProps['setDefaultBaseUrlDefinition'];
  onDeleteBaseUrl: (def: { id: string; name: string }) => void;
}

export function EnvBody({
  env,
  projectId,
  modules,
  setEnvironmentVar,
  renameEnvironmentVar,
  setEnvironmentModuleBaseUrl,
  onAddVar,
  baseUrlDefinitions,
  defaultBaseUrlDefinitionId,
  addBaseUrlDefinition,
  updateBaseUrlDefinition,
  setDefaultBaseUrlDefinition,
  onDeleteBaseUrl,
}: EnvBodyProps) {
  return (
    <div className="flex flex-col">
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
