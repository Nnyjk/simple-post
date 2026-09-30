/**
 * CRUD sub-factory — project-scoped baseUrl definitions.
 *
 * One of five sub-factories composed by `createCrudSlice` (see
 * `../crud-slice.ts` for the slice-wide contract: optimistic `set()` →
 * `invokeCmd` → snapshot-and-rollback).
 *
 * Definitions live on the project (`project.baseUrlDefinitions`), while
 * the per-environment overrides live in `environment.baseUrls` keyed by
 * definition id. Deleting a definition therefore has to null out the
 * endpoints pointing at it *and* leave the env overrides alone — see the
 * comment on `deleteBaseUrlDefinition` for why the orphaned overrides are
 * not cleaned up.
 */
import type { StateCreator } from 'zustand';
import type { BaseUrlDefinition, EndpointDetail, Project } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { uid } from '@/lib/utils';
import type { AppState } from '../app-store';
import { invokeWithRollback } from './crud-helpers';

/** baseUrl-definition actions owned by the CRUD slice. */
export interface BaseUrlCrudActions {
  addBaseUrlDefinition: (projectId: string, name: string) => string;
  updateBaseUrlDefinition: (
    projectId: string,
    defId: string,
    patch: Partial<Pick<BaseUrlDefinition, 'name' | 'description'>>,
  ) => Promise<void>;
  deleteBaseUrlDefinition: (projectId: string, defId: string) => Promise<void>;
  setDefaultBaseUrlDefinition: (projectId: string, defId: string | null) => Promise<void>;
}

export const createBaseUrlCrud: StateCreator<AppState, [], [], BaseUrlCrudActions> = (
  set,
  get,
) => ({
  /**
   * Optimistic local add + background invoke. Returns the temp id
   * synchronously because EnvironmentSection passes it directly into
   * `setEnvironmentBaseUrl` to seed an empty row. On invoke success
   * the temp row is replaced with the persisted entity (server may
   * assign a different id); on failure the local row is removed.
   */
  addBaseUrlDefinition: (projectId, name) => {
    const defId = uid();
    set((s) => ({
      projects: s.projects.map((p) => {
        if (p.id !== projectId) return p;
        const wasEmpty = p.baseUrlDefinitions.length === 0;
        return {
          ...p,
          baseUrlDefinitions: [
            ...p.baseUrlDefinitions,
            { id: defId, name: name.trim() || '未命名' },
          ],
          // Convenience: when the project had no defs yet, the newly
          // added one becomes the default — otherwise the picker
          // would have nothing to show as "项目默认".
          defaultBaseUrlDefinitionId: wasEmpty ? defId : p.defaultBaseUrlDefinitionId,
        };
      }),
    }));
    invokeCmd<BaseUrlDefinition>(COMMANDS.baseUrlDefsCreate, {
      projectId,
      name: name.trim() || '未命名',
    })
      .then((persisted) => {
        set((s) => ({
          projects: s.projects.map((p) => {
            if (p.id !== projectId) return p;
            return {
              ...p,
              baseUrlDefinitions: p.baseUrlDefinitions.map((d) => (d.id === defId ? persisted : d)),
            };
          }),
        }));
      })
      .catch((e) => {
        console.error('[store] base_url_defs_create failed', e);
        set((s) => ({
          projects: s.projects.map((p) => {
            if (p.id !== projectId) return p;
            return {
              ...p,
              baseUrlDefinitions: p.baseUrlDefinitions.filter((d) => d.id !== defId),
            };
          }),
        }));
      });
    return defId;
  },
  updateBaseUrlDefinition: async (projectId, defId, patch) => {
    const prevProject = get().projects.find((p) => p.id === projectId);
    const prevDef = prevProject?.baseUrlDefinitions.find((d) => d.id === defId);
    if (!prevProject || !prevDef) return;
    const optimistic: Project = {
      ...prevProject,
      baseUrlDefinitions: prevProject.baseUrlDefinitions.map((d) =>
        d.id === defId ? { ...d, ...patch } : d,
      ),
    };
    set((s) => ({
      projects: s.projects.map((p) => (p.id === projectId ? optimistic : p)),
    }));
    await invokeWithRollback(
      'base_url_defs_update',
      () => invokeCmd<BaseUrlDefinition>(COMMANDS.baseUrlDefsUpdate, { id: defId, patch }),
      () =>
        set((s) => ({
          projects: s.projects.map((p) => (p.id === projectId ? prevProject : p)),
        })),
    );
  },
  deleteBaseUrlDefinition: async (projectId, defId) => {
    const snapshot = {
      projects: get().projects,
      endpointDetails: get().endpointDetails,
    };
    set((s) => {
      // Cascade: any endpoint that pointed at this def falls back to
      // the project default. We don't bother removing the now-orphan
      // entry from each env's baseUrls map — the resolution function
      // never reads an entry under a dead defId, so the stale data
      // is harmless and self-cleans on next save.
      const nextDetails: Record<string, EndpointDetail> = {};
      for (const [id, d] of Object.entries(s.endpointDetails)) {
        nextDetails[id] = d.baseUrlDefinitionId === defId ? { ...d, baseUrlDefinitionId: null } : d;
      }
      return {
        projects: s.projects.map((p) => {
          if (p.id !== projectId) return p;
          return {
            ...p,
            baseUrlDefinitions: p.baseUrlDefinitions.filter((d) => d.id !== defId),
            defaultBaseUrlDefinitionId:
              p.defaultBaseUrlDefinitionId === defId ? null : p.defaultBaseUrlDefinitionId,
          };
        }),
        endpointDetails: nextDetails,
      };
    });
    await invokeWithRollback(
      'base_url_defs_delete',
      () => invokeCmd<void>(COMMANDS.baseUrlDefsDelete, { id: defId }),
      () => set(snapshot),
    );
  },
  setDefaultBaseUrlDefinition: async (projectId, defId) => {
    const prevProject = get().projects.find((p) => p.id === projectId);
    if (!prevProject) return;
    const optimistic: Project = { ...prevProject, defaultBaseUrlDefinitionId: defId };
    set((s) => ({
      projects: s.projects.map((p) => (p.id === projectId ? optimistic : p)),
    }));
    await invokeWithRollback(
      'base_url_defs_set_default',
      () => invokeCmd<Project>(COMMANDS.baseUrlDefsSetDefault, { projectId, defId }),
      () =>
        set((s) => ({
          projects: s.projects.map((p) => (p.id === projectId ? prevProject : p)),
        })),
    );
  },
});
