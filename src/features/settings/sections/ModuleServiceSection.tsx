/**
 * ModuleServiceSection — the 「服务」 layer of a **module** tab.
 *
 * WHAT IT ANSWERS, AND WHY IT ISN'T `EnvironmentSection`
 * ------------------------------------------------------
 * The project's 环境 section answers "what are this project's environments,
 * and what is every module's address in each of them?" — a cartesian
 * product, with the module as a grouping column. Inside one module's own
 * tab that question is already answered by the pane's title, and the
 * grouping column would print the same name on every row.
 *
 * So this is the same table with the module axis collapsed to one entry:
 * every environment, and for each one, this module's address for each of
 * the project's services. It is still `ModuleBaseUrlTable` in `module`
 * mode — the cell semantics (a value belongs to exactly one
 * module × service × environment, with nothing to inherit from) are the
 * whole reason that component exists, and a second table would be a second
 * place for that rule to live.
 *
 * WHAT IT DELIBERATELY DOES NOT OFFER
 * -----------------------------------
 * The service *list* belongs to the project. Renaming, starring, deleting
 * or adding a service is a project-level edit, so none of those controls
 * appear here: the cells are the module's to fill, the catalogue is the
 * project's to curate. When there is nothing to fill — no environments, or
 * no services yet — the empty state says where the missing piece is created
 * and opens the project tab, which is where both of them live.
 *
 * Environment creation / deletion are excluded for the same reason; the
 * env strip is still `EnvSwitcher`, so the list reads identically, just
 * without the two project-level buttons.
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

  // The module may belong to a project that is not the one in front — a
  // tab set is per project, but the module is found through the flat node
  // list, and `rootId` is the key that says which project owns it.
  const project = useMemo(
    () => (mod ? (projects.find((p) => p.id === mod.rootId) ?? null) : null),
    [projects, mod],
  );

  const projectEnvs = useMemo(
    () => (mod ? environments.filter((e) => e.projectId === mod.rootId) : []),
    [environments, mod],
  );

  // Default to the project-active env whenever the list shape changes,
  // unless the user has explicitly picked one. Same rule as
  // `EnvironmentSection`, so the two strips behave identically.
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

  /** Environments and services are both created from the project tab. */
  const goToProject = () => openTab('project', mod.rootId);

  return (
    <div className="space-y-3">
      <EnvSwitcher
        environments={projectEnvs}
        activeId={activeId}
        onPick={setActiveId}
        // No-ops: both controls are hidden on this strip, but the props
        // are required so a future `showDelete` flip here cannot reach an
        // undefined callback.
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
              // Keyed by env id so switching environments resets this
              // table's own transient state — an input left mid-edit on
              // the previous env cannot bleed across.
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
