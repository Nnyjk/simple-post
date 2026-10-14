/**
 * CRUD sub-factory — project-scoped baseUrl service definitions.
 *
 * One of five sub-factories composed by `createCrudSlice` (see
 * `../crud-slice.ts` for the slice-wide contract: optimistic `set()` →
 * `invokeCmd` → snapshot-and-rollback).
 *
 * A service belongs to the **project**, not to a module — every module
 * sees the same list, so these actions are all keyed by `projectId`. The
 * per-module part of the model is the URL, which lives in
 * `environment.moduleBaseUrls[defId][moduleId]` and is written by
 * `crud-environment.ts`.
 *
 * Deleting a definition still has to null out the endpoints pointing at
 * it; see the comment on `deleteBaseUrlDefinition` for why the orphaned
 * per-env URLs are not cleaned up.
 */
import type { StateCreator } from 'zustand';
import type { BaseUrlDefinition, EndpointDetail, Project } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { uid } from '@/lib/utils';
import type { AppState } from '../app-store';
import { invokeWithRollback } from './crud-helpers';

/** baseUrl-definition actions owned by the CRUD slice. */
export interface BaseUrlCrudActions {
  /**
   * Add a service to `projectId`. Returns its id synchronously so the
   * caller can seed an env cell for it without a round trip.
   */
  addBaseUrlDefinition: (projectId: string, name: string) => string;
  updateBaseUrlDefinition: (
    projectId: string,
    defId: string,
    patch: Partial<Pick<BaseUrlDefinition, 'name' | 'description'>>,
  ) => Promise<void>;
  deleteBaseUrlDefinition: (projectId: string, defId: string) => Promise<void>;
  /**
   * Star a service as the **project's** default, or clear the star with
   * `null`. The star is project-wide — there is no per-module default —
   * so it is what an endpoint with no `baseUrlDefinitionId` resolves
   * through. Clearing it is a real (if intentional) way to leave those
   * endpoints with no baseUrl.
   */
  setDefaultBaseUrlDefinition: (projectId: string, defId: string | null) => Promise<void>;
}

export const createBaseUrlCrud: StateCreator<AppState, [], [], BaseUrlCrudActions> = (
  set,
  get,
) => ({
  /**
   * Optimistic local add + background invoke. Returns the id
   * synchronously because the settings table passes it straight into
   * `setEnvironmentModuleBaseUrl` to seed an env cell.
   *
   * The id is minted **here** and the backend adopts it (`opts.id`), which
   * is the whole point. Two things in this slice are keyed by a service id
   * before the row is ever persisted — the project's ★ and every
   * `env.moduleBaseUrls[defId][moduleId]` cell — so a backend-minted id
   * turns both into keys that name a row nobody has: the cell write has
   * already gone to the database under the old key, and swapping the row
   * afterwards cannot move it. `resolveEndpointBaseUrl` then reads a cell
   * that does not exist and yields `''`, which is why a brand-new service
   * could be selected but never applied. `duplicateEndpoint` already
   * mints its id this way for the same class of reason.
   *
   * The project's first service becomes its default here. The backend
   * does the same, but mirroring it locally is what stops the ★ from
   * appearing a moment late in the one place it is load-bearing: an
   * endpoint that picked no service resolves through it.
   */
  addBaseUrlDefinition: (projectId, name) => {
    const defId = uid();
    const label = name.trim() || '未命名';
    set((s) => ({
      projects: s.projects.map((p) => {
        if (p.id !== projectId) return p;
        const wasEmpty = p.baseUrlDefinitions.length === 0;
        return {
          ...p,
          baseUrlDefinitions: [...p.baseUrlDefinitions, { id: defId, name: label }],
          // Convenience: with no ★ the picker would have nothing to
          // offer as the project default.
          defaultBaseUrlDefinitionId: wasEmpty ? defId : p.defaultBaseUrlDefinitionId,
        };
      }),
    }));
    invokeCmd<BaseUrlDefinition>(COMMANDS.baseUrlDefsCreate, {
      projectId,
      name: label,
      opts: { id: defId },
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
              // The row is gone, so a ★ naming it is a dangling pointer:
              // `resolveEndpointBaseUrl` would look it up, miss, and hand
              // every endpoint in the project an empty base URL — a
              // failure the create attempt already caused once. Take the
              // ★ back to nothing rather than promote a service the user
              // never chose.
              defaultBaseUrlDefinitionId:
                p.defaultBaseUrlDefinitionId === defId ? null : p.defaultBaseUrlDefinitionId,
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
      // Cascade: any endpoint that pointed at this def falls back to the
      // project default. We don't bother removing the now-orphan cells
      // from each env's `moduleBaseUrls` — the resolution function never
      // reads an entry under a dead defId, so the stale data is harmless
      // and self-cleans on next save.
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
            // The ★ pointed at a row that no longer exists; leaving it
            // would make the settings table highlight nothing and the
            // resolver look a def up that misses forever.
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
    // One field, one `set()`: the star is a single id rather than a
    // flag spread across rows, so there is no intermediate state in which
    // two services are both default.
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
