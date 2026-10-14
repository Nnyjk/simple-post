/**
 * ModuleServiceSection —— **模块** 标签下的「服务」层。
 *
 * 它回答什么，以及为何不是 `EnvironmentSection`
 * ------------------------------------------------
 * 项目的「环境」分区回答的是「本项目有哪些环境，每个模块在各环境里的地址是什么」——
 * 一个以模块为分组列的笛卡尔积。在某个模块自己的标签里，这个问题已经被面板标题回答过了，
 * 留着分组列只会在每一行上重复打印同一个名字。
 *
 * 所以这就是同一张表，只是把模块这一维塌成一项：列出每个环境，以及本模块在各环境下、
 * 对项目每个服务的地址。它仍是 `ModuleBaseUrlTable` 的 `module` 模式 —— 单元格语义
 * （一个值只属于唯一的 模块 × 服务 × 环境，且无可继承）正是该组件存在的全部理由，
 * 再来一张表就等于让这条规则有第二个住处。
 *
 * 它刻意不提供什么
 * ----------------
 * 服务*清单*属于项目。改名、打星号、删除、新增都是项目级编辑，故这里一个控件都不出现：
 * 单元格归模块填，清单归项目维护。没什么可填时 —— 还没有环境，或还没有服务 —— 空状态会
 * 说明缺的那块在哪里创建，并打开项目标签，两者都在那里。
 *
 * 环境的新建 / 删除同样排除，理由一致；环境条仍是 `EnvSwitcher`，故列表读起来完全一样，
 * 只是少了那两个项目级按钮。
 */

import { useEffect, useMemo, useState } from 'react';
import { Globe } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { Button } from '@/components/ui/button';
import { EmptyHint } from '@/components/ui/empty-hint';
import { EnvSwitcher } from './rows/EnvSwitcher';
import { GroupHeader } from './rows/GroupHeader';
import { ModuleBaseUrlTable } from './rows/ModuleBaseUrlTable';

export function ModuleServiceSection({ moduleId }: { moduleId: string }) {
  const mod = useAppStore((s) => s.nodes.find((n) => n.id === moduleId));
  const environments = useAppStore((s) => s.environments);
  const projects = useAppStore((s) => s.projects);
  const setEnvironmentModuleBaseUrl = useAppStore((s) => s.setEnvironmentModuleBaseUrl);
  const addBaseUrlDefinition = useAppStore((s) => s.addBaseUrlDefinition);
  const updateBaseUrlDefinition = useAppStore((s) => s.updateBaseUrlDefinition);
  const setDefaultBaseUrlDefinition = useAppStore((s) => s.setDefaultBaseUrlDefinition);
  const openTab = useAppStore((s) => s.openTab);

  const [activeId, setActiveId] = useState<string | null>(null);

  // 该模块可能属于另一个项目：标签集按项目分，但模块是从打平的节点列表里找到的，
  // `rootId` 才是说明它归哪个项目的那把钥匙。
  const project = useMemo(
    () => (mod ? (projects.find((p) => p.id === mod.rootId) ?? null) : null),
    [projects, mod],
  );

  const projectEnvs = useMemo(
    () => (mod ? environments.filter((e) => e.projectId === mod.rootId) : []),
    [environments, mod],
  );

  // 列表形状一变就回到项目当前激活的环境，除非用户已明确选过。规则同
  // `EnvironmentSection`，两条环境条行为一致。
  useEffect(() => {
    if (projectEnvs.length === 0) {
      setActiveId(null);
      return;
    }
    if (activeId && projectEnvs.some((e) => e.id === activeId)) return;
    const active = projectEnvs.find((e) => e.isActive);
    setActiveId(active?.id ?? projectEnvs[0]!.id);
  }, [projectEnvs, activeId]);

  const activeEnv = useMemo(
    () => projectEnvs.find((e) => e.id === activeId) ?? null,
    [projectEnvs, activeId],
  );

  if (!mod) {
    return <EmptyHint className="px-3 py-8 text-xs">该模块已不存在</EmptyHint>;
  }

  /** 环境和服务都在项目标签里创建。 */
  const goToProject = () => openTab('project', mod.rootId);

  return (
    <div className="space-y-3">
      <EnvSwitcher
        environments={projectEnvs}
        activeId={activeId}
        onPick={setActiveId}
        // 空操作：这条环境条上两个控件都隐藏了，但 props 仍是必填，日后有人在这里翻
        // `showDelete` 也不会调到 undefined 回调。
        onRequestDelete={() => undefined}
        onAdd={() => undefined}
        showAdd={false}
        showDelete={false}
      />

      {projectEnvs.length === 0 ? (
        <EmptyHint className="rounded-lg border-border bg-card/30 px-3 py-8 text-xs">
          <span className="block">该项目还没有环境</span>
          <span className="mt-1 block text-muted-foreground/80">
            环境属于项目，先在项目标签里创建第一个，这里才会有可填的地址。
          </span>
          <Button variant="outline" size="sm" className="mt-3 h-7 text-xs" onClick={goToProject}>
            打开项目标签
          </Button>
        </EmptyHint>
      ) : (
        activeEnv && (
          <div className="overflow-hidden rounded-lg border border-border/70 bg-card/40 shadow-[0_1px_0_0_rgba(255,255,255,0.02)_inset]">
            <GroupHeader
              icon={<Globe className="h-3.5 w-3.5" />}
              title="前置 URL"
              description="服务清单属于项目，这里填的是本模块自己的地址"
              meta={`${project?.baseUrlDefinitions.length ?? 0} 个服务`}
            />
            <ModuleBaseUrlTable
              // 以 env id 为 key，切环境即重置这张表自己的临时状态 —— 上一个环境里
              // 编辑到一半的输入不会渗过来。
              key={activeEnv.id}
              mode="module"
              env={activeEnv}
              projectId={mod.rootId}
              modules={[mod]}
              baseUrlDefinitions={project?.baseUrlDefinitions ?? []}
              defaultBaseUrlDefinitionId={project?.defaultBaseUrlDefinitionId ?? null}
              setEnvironmentModuleBaseUrl={setEnvironmentModuleBaseUrl}
              addBaseUrlDefinition={addBaseUrlDefinition}
              updateBaseUrlDefinition={updateBaseUrlDefinition}
              setDefaultBaseUrlDefinition={setDefaultBaseUrlDefinition}
              onDeleteBaseUrl={() => undefined}
            />
          </div>
        )
      )}
    </div>
  );
}
