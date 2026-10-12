/**
 * ProjectEnvSection — the store-connected host for {@link EnvironmentSection}.
 *
 * WHY THIS EXISTS
 * ---------------
 * `EnvironmentSection` is presentational by design: every list and every
 * action arrives as a prop, which is what lets its rows be tested without a
 * store. That made it cheap to reuse — and then expensive to reuse, because
 * the ~15 props have to be assembled from the store by whoever mounts it,
 * and the assembly is where the project's own rules live:
 *
 *   - environments are filtered to `activeProjectId`
 *   - the 前置 URL table's row axis is *this project's modules*, sorted by
 *     `sortOrder` because `nodes` is a flat list whose array order is not
 *     the tree's sibling order
 *   - the service list and the project ★ come off the project row
 *
 * Those rules now have two hosts — the settings tab's 环境 section, and the
 * project tab's own 环境 layer — and a rule written twice is a rule that
 * drifts. So the wiring lives here, once, and each host mounts *this*.
 *
 * The no-project guard is here for the same reason. Both hosts are
 * reachable with no project open (the workspace's settings tab, and the
 * welcome screen's `SettingsSurface`), and the section renders a
 * working-looking "新建环境" button whose write would go to
 * `projectId === ''` and silently fail. Say what is missing instead.
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

  // Modules are the row axis of the 前置 URL table — the row set is
  // (every module × every project service) — so they are filtered to this
  // project and sorted by `sortOrder`, which the table presents as tree
  // order while `nodes` is a flat list.
  const modules = useMemo(
    () =>
      nodes
        .filter((n) => n.kind === 'module' && n.rootId === activeProjectId)
        .sort((a, b) => a.sortOrder - b.sortOrder),
    [nodes, activeProjectId],
  );

  if (activeProjectId === '') {
    return (
      // `px-12` matches `SettingsCategory`'s own inset, so this hint sits on
      // the same reading column as the section it stands in for — in both
      // hosts.
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
