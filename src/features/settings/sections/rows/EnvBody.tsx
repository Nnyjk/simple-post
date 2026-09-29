import { Braces, Globe } from 'lucide-react';
import type { Environment } from '@/types/domain';
import type { EnvironmentSectionProps } from '../EnvironmentSection';
import { BaseUrlTable } from './BaseUrlTable';
import { EnvHeader } from './EnvHeader';
import { GroupHeader } from './GroupHeader';
import { VariableTable } from './VariableTable';

/**
 * EnvBody — one env's tables (URL rows + variable rows).
 */
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

export function EnvBody({
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
