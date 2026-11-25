import { Braces, Globe } from 'lucide-react';
import { useRef } from 'react';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';
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
  // 环境一换，宿主就以 env id 为 key 把本组件整棵重挂，行数据是一次性替换的 —— 所以动画
  // 挂在根容器上，而不是每一行：项目可能有几十个模块，逐行补间既是性能问题，也让眼睛跟着
  // 一排行一起动，读不出「换的是整张表」。补间结束后清掉 transform，免得给表格里任何
  // fixed / sticky 元素留下一个非视口的包含块。
  const rootRef = useRef<HTMLDivElement>(null);
  useGSAP(
    () => {
      const el = rootRef.current;
      if (!el) return;
      gsap.killTweensOf(el);
      return motionSafe(() => {
        gsap.fromTo(
          el,
          { opacity: 0, y: 4 },
          { opacity: 1, y: 0, duration: 0.2, ease: 'power2.out', clearProps: 'transform' },
        );
      });
    },
    { scope: rootRef, dependencies: [env.id] },
  );

  return (
    <div ref={rootRef} className="flex flex-col">
      <GroupHeader
        icon={<Globe className="h-3.5 w-3.5" />}
        title="模块地址"
        // 按实际的样子描述这个模型：每个模块在本环境里各有一份地址与一组有序的路径改写规则。
        description="每个模块在本环境里各有一份地址与一组有序的路径改写规则，按顺序匹配、命中首条即停止"
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
