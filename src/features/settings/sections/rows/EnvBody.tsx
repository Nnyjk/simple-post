import { Braces, Globe } from 'lucide-react';
import type { Environment, ResourceNode } from '@/types/domain';
import type { EnvironmentSectionProps } from '../EnvironmentSection';
import { GroupHeader } from './GroupHeader';
import { ModuleBaseUrlTable } from './ModuleBaseUrlTable';
import { VariableTable } from './VariableTable';

/**
 * EnvBody —— 单个环境的各张表（前置 URL 行 + 变量行）。
 *
 * 没有名字行。上方的切换器已经说明了当前打开的是哪个环境并持有重命名；在这里重复一遍，
 * 正是让项目 / 设置里的环境视图看起来与模块那边不一致的原因 —— 而模块那边从来没有这一行。
 * 以 env id 为 key，故每个环境各自重挂载。
 */
interface EnvBodyProps {
  env: Environment;
  projectId: string;
  /** 本项目的模块 —— 前置 URL 表的分组轴。 */
  modules: readonly ResourceNode[];

  setEnvironmentVar: EnvironmentSectionProps['setEnvironmentVar'];
  setEnvironmentVarDisabled: EnvironmentSectionProps['setEnvironmentVarDisabled'];
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
  setEnvironmentVarDisabled,
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
        // 按实际的样子描述这个模型：服务清单属于项目、每个模块都看得见全部，而第三列的
        // 地址是本模块在此环境下对该服务自己的取值。
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
        setEnvironmentVarDisabled={setEnvironmentVarDisabled}
        renameEnvironmentVar={renameEnvironmentVar}
        onAddVar={onAddVar}
      />
    </div>
  );
}
