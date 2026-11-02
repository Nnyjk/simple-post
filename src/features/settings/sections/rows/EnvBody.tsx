import { Braces, Globe } from 'lucide-react';
import type { Environment, ResourceNode } from '@/types/domain';
import type { EnvironmentSectionProps } from '../EnvironmentSection';
import { GroupHeader } from './GroupHeader';
import { ModuleBaseUrlTable } from './ModuleBaseUrlTable';
import { VariableTable } from './VariableTable';

/**
 * EnvBody —— 单个环境的各张表（模块地址行 + 变量行）。
 *
 * 没有名字行。上方的切换器已经说明了当前打开的是哪个环境并持有重命名；在这里重复一遍，
 * 正是让项目 / 设置里的环境视图看起来与模块那边不一致的原因 —— 而模块那边从来没有这一行。
 * 以 env id 为 key，故每个环境各自重挂载。
 */
interface EnvBodyProps {
  env: Environment;
  /** 本项目的模块 —— 模块地址表的行轴。 */
  modules: readonly ResourceNode[];

  setEnvironmentVar: EnvironmentSectionProps['setEnvironmentVar'];
  setEnvironmentVarDisabled: EnvironmentSectionProps['setEnvironmentVarDisabled'];
  renameEnvironmentVar: EnvironmentSectionProps['renameEnvironmentVar'];
  setEnvironmentModuleBaseUrl: EnvironmentSectionProps['setEnvironmentModuleBaseUrl'];
  onAddVar: () => void;
}

export function EnvBody({
  env,
  modules,
  setEnvironmentVar,
  setEnvironmentVarDisabled,
  renameEnvironmentVar,
  setEnvironmentModuleBaseUrl,
  onAddVar,
}: EnvBodyProps) {
  return (
    <div className="flex flex-col">
      <GroupHeader
        icon={<Globe className="h-3.5 w-3.5" />}
        title="模块地址"
        // 按实际的样子描述这个模型：每个模块在本环境里各有一份地址与代理地址。
        description="每个模块在本环境里各有一份地址与代理地址"
        meta={modules.length > 0 ? `${modules.length} 个模块` : undefined}
      />
      <ModuleBaseUrlTable
        env={env}
        modules={modules}
        setEnvironmentModuleBaseUrl={setEnvironmentModuleBaseUrl}
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
