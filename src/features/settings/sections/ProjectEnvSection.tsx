/**
 * ProjectEnvSection —— {@link EnvironmentSection} 的接 store 的宿主。
 *
 * 它为何存在
 * ----------
 * `EnvironmentSection` 按设计是纯展示：每个列表和每个动作都从 props 进来，它的行因此
 * 能脱离 store 测试。但那 15 个左右的 props 得由挂载方从 store 里拼出来，而这份拼装
 * 正是项目自身规则的所在：
 *
 *   - 环境按 `activeProjectId` 过滤
 *   - 前置 URL 表的行轴是*本项目的模块*，按 `sortOrder` 排序，因为 `nodes` 是打平的
 *     列表，数组顺序不是树的兄弟顺序
 *   - 服务清单与项目 ★ 取自项目行
 *
 * 这些规则现在有两个宿主：设置标签的环境分区，和项目标签自己的环境层。规则写两遍就会
 * 漂移，故接线只在此处写一次，两个宿主挂的都是*这个*。
 *
 * 无项目守卫同理。两个宿主在没有项目打开时也都可达（工作区的设置标签，以及欢迎页的
 * `SettingsSurface`），而该分区会渲染一个看着能用的「新建环境」按钮，其写入却会打到
 * `projectId === ''` 并静默失败。故这里直接说明缺了什么。
 */

import { useMemo } from 'react';
import { useAppStore } from '@/stores/app-store';
import { EmptyHint } from '@/components/ui/empty-hint';
import { EnvironmentSection } from './EnvironmentSection';

export function ProjectEnvSection() {
  const allEnvironments = useAppStore((s) => s.environments);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const projects = useAppStore((s) => s.projects);
  const nodes = useAppStore((s) => s.nodes);

  const setEnvironmentVar = useAppStore((s) => s.setEnvironmentVar);
  const renameEnvironmentVar = useAppStore((s) => s.renameEnvironmentVar);
  const setEnvironmentModuleBaseUrl = useAppStore((s) => s.setEnvironmentModuleBaseUrl);
  const deleteEnvironment = useAppStore((s) => s.deleteEnvironment);
  const updateEnvironment = useAppStore((s) => s.updateEnvironment);
  const addEnvironment = useAppStore((s) => s.addEnvironment);

  const addBaseUrlDefinition = useAppStore((s) => s.addBaseUrlDefinition);
  const updateBaseUrlDefinition = useAppStore((s) => s.updateBaseUrlDefinition);
  const deleteBaseUrlDefinition = useAppStore((s) => s.deleteBaseUrlDefinition);
  const setDefaultBaseUrlDefinition = useAppStore((s) => s.setDefaultBaseUrlDefinition);

  const activeProject = useMemo(
    () => projects.find((p) => p.id === activeProjectId) ?? null,
    [projects, activeProjectId],
  );

  const environments = useMemo(
    () => allEnvironments.filter((e) => e.projectId === activeProjectId),
    [allEnvironments, activeProjectId],
  );

  // 模块是前置 URL 表的行轴，行集是（每个模块 × 每个项目服务），故按本项目过滤并按
  // `sortOrder` 排序 —— 表里按树序呈现，而 `nodes` 是打平的。
  const modules = useMemo(
    () =>
      nodes
        .filter((n) => n.kind === 'module' && n.rootId === activeProjectId)
        .sort((a, b) => a.sortOrder - b.sortOrder),
    [nodes, activeProjectId],
  );

  if (activeProjectId === '') {
    return (
      // `px-12` 与 `SettingsCategory` 自身的内缩一致，故这条提示与它所代表的分区落在
      // 同一条阅读列上，两个宿主皆然。
      <EmptyHint className="px-12 py-10 text-xs">
        环境变量属于项目。请先在欢迎页选择一个项目，再回到这里管理它的环境与前置 URL。
      </EmptyHint>
    );
  }

  return (
    <EnvironmentSection
      environments={environments}
      projectId={activeProjectId}
      modules={modules}
      setEnvironmentVar={setEnvironmentVar}
      renameEnvironmentVar={renameEnvironmentVar}
      setEnvironmentModuleBaseUrl={setEnvironmentModuleBaseUrl}
      deleteEnvironment={deleteEnvironment}
      updateEnvironment={updateEnvironment}
      addEnvironment={addEnvironment}
      baseUrlDefinitions={activeProject?.baseUrlDefinitions ?? []}
      defaultBaseUrlDefinitionId={activeProject?.defaultBaseUrlDefinitionId ?? null}
      addBaseUrlDefinition={addBaseUrlDefinition}
      updateBaseUrlDefinition={updateBaseUrlDefinition}
      deleteBaseUrlDefinition={deleteBaseUrlDefinition}
      setDefaultBaseUrlDefinition={setDefaultBaseUrlDefinition}
    />
  );
}
