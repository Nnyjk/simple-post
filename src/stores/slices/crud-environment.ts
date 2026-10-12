/**
 * CRUD sub-factory — environments.
 *
 * One of five sub-factories composed by `createCrudSlice` (see
 * `../crud-slice.ts` for the slice-wide contract: optimistic `set()` →
 * `invokeCmd` → snapshot-and-rollback).
 *
 * `null` is the delete sentinel on both a variable and a baseUrl slot, and
 * an empty string is a real value — the tests in
 * `../../__tests__/crud-slice.test.ts` pin that distinction, so the
 * `value === null` checks below are load-bearing, not style.
 *
 * `moduleBaseUrls[defId][moduleId]` is the only place a service's URL
 * lives: one independent cell per (module, service) pair, no inheritance
 * and no fallback. `resolveEndpointBaseUrl` reads exactly one of them.
 * The flat `baseUrls` map is deprecated and deliberately not written.
 *
 * `deleteEnvironment` and `setEnvironmentActive` write keys owned by
 * other slices (`activeEnvironmentId`, `endpointDetails`) in the same
 * atomic `set()` as their own. Splitting those into separate calls would
 * change what the IPC-failure rollback restores.
 */
import type { StateCreator } from 'zustand';
import type { Environment, EndpointDetail } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { AppState } from '../app-store';
import { invokeWithRollback } from './crud-helpers';

/** Environment actions owned by the CRUD slice. */
export interface EnvironmentCrudActions {
  setEnvironmentVar: (envId: string, key: string, value: string | null) => Promise<void>;
  renameEnvironmentVar: (envId: string, fromKey: string, toKey: string) => Promise<void>;
  addEnvironment: (projectId: string, name: string) => Promise<string | null>;
  updateEnvironment: (envId: string, patch: Partial<Pick<Environment, 'name'>>) => Promise<void>;
  /**
   * Set (or with `null` clear) one cell: what service `defId` points at,
   * for module `moduleId`, in this environment. Each cell is an
   * independent value — clearing one must not touch its neighbours.
   */
  setEnvironmentModuleBaseUrl: (
    envId: string,
    defId: string,
    moduleId: string,
    url: string | null,
  ) => Promise<void>;
  deleteEnvironment: (envId: string) => Promise<void>;
  setEnvironmentActive: (envId: string) => Promise<void>;
}

export const createEnvironmentCrud: StateCreator<AppState, [], [], EnvironmentCrudActions> = (
  set,
  get,
) => ({
  setEnvironmentVar: async (envId, key, value) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    const variables = { ...prev.variables };
    if (value === null) delete variables[key];
    else variables[key] = value;
    const optimistic: Environment = {
      ...prev,
      variables,
    };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    await invokeWithRollback(
      'environments_set_var',
      () => invokeCmd<Environment>(COMMANDS.environmentsSetVar, { envId, key, value }),
      () =>
        set((s) => ({
          environments: s.environments.map((env) => (env.id === envId ? prev : env)),
        })),
    );
  },
  renameEnvironmentVar: async (envId, fromKey, toKey) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    if (fromKey === toKey) return;

    // Optimistic single-step swap: the old key disappears and the new
    // one appears in the same state write, so no intermediate render
    // ever shows the variable missing. The value moves with the key.
    const { [fromKey]: value, ...rest } = prev.variables;
    if (value === undefined) return;
    const optimistic: Environment = {
      ...prev,
      variables: { ...rest, [toKey]: value },
    };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));

    try {
      // One command, one transaction. The previous shape — two
      // `environments_set_var` calls — left the variable deleted for
      // the window between them, and if the second call failed the
      // rollback below restored only the UI while the backend had
      // already committed the delete.
      const updated = await invokeCmd<Environment>(COMMANDS.environmentsRenameVar, {
        envId,
        fromKey,
        toKey,
      });
      // The backend echoes the authoritative post-rename env, so a
      // concurrent edit from another window isn't clobbered by the
      // optimistic guess.
      set((s) => ({
        environments: s.environments.map((env) => (env.id === envId ? updated : env)),
      }));
    } catch (e) {
      console.error('[store] environments_rename_var failed', e);
      set((s) => ({
        environments: s.environments.map((env) => (env.id === envId ? prev : env)),
      }));
    }
  },
  addEnvironment: async (projectId, name) => {
    try {
      const env = await invokeCmd<Environment>(COMMANDS.environmentsCreate, { projectId, name });
      set((s) => ({ environments: [...s.environments, env] }));
      return env.id;
    } catch (e) {
      console.error('[store] environments_create failed', e);
      return null;
    }
  },
  updateEnvironment: async (envId, patch) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    const optimistic: Environment = { ...prev, ...patch };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    await invokeWithRollback(
      'environments_update',
      () => invokeCmd<Environment>(COMMANDS.environmentsUpdate, { id: envId, patch }),
      () =>
        set((s) => ({
          environments: s.environments.map((env) => (env.id === envId ? prev : env)),
        })),
    );
  },
  setEnvironmentModuleBaseUrl: async (envId, defId, moduleId, url) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    // Copy both levels: the inner map is shared with every other module's
    // row, so mutating it in place would write this cell into modules the
    // caller never named.
    const nextByModule = { ...(prev.moduleBaseUrls[defId] ?? {}) };
    if (url === null) delete nextByModule[moduleId];
    else nextByModule[moduleId] = url;
    const nextModuleBaseUrls = { ...prev.moduleBaseUrls, [defId]: nextByModule };
    // Drop the whole service row once its last cell is gone, so an env
    // that has been cleared does not carry a hollow `defId → {}` shell
    // that reads as "this service is configured here" in any UI diffing
    // the object by key.
    if (Object.keys(nextByModule).length === 0) delete nextModuleBaseUrls[defId];
    const optimistic: Environment = { ...prev, moduleBaseUrls: nextModuleBaseUrls };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    await invokeWithRollback(
      'environments_set_module_baseurl',
      () =>
        invokeCmd<Environment>(COMMANDS.environmentsSetModuleBaseurl, {
          envId,
          defId,
          moduleId,
          url,
        }),
      () =>
        set((s) => ({
          environments: s.environments.map((env) => (env.id === envId ? prev : env)),
        })),
    );
  },
  deleteEnvironment: async (envId) => {
    const snapshot = {
      environments: get().environments,
      endpointDetails: get().endpointDetails,
      activeEnvironmentId: get().activeEnvironmentId,
    };
    set((s) => {
      // Clear `environmentId` on any endpoint that pointed at the deleted
      // env so it stops resolving against it. We don't have a selector
      // that re-resolves baseUrl on the fly, so the explicit null-out is
      // what keeps the URL bar honest.
      const nextDetails: Record<string, EndpointDetail> = {};
      for (const [id, d] of Object.entries(s.endpointDetails)) {
        nextDetails[id] = d.environmentId === envId ? { ...d, environmentId: null } : d;
      }
      return {
        environments: s.environments.filter((env) => env.id !== envId),
        endpointDetails: nextDetails,
        // If the active environment was the one we just deleted, fall
        // back to the next sibling (or the project default).
        activeEnvironmentId: s.activeEnvironmentId === envId ? '' : s.activeEnvironmentId,
      };
    });
    await invokeWithRollback(
      'environments_delete',
      () => invokeCmd<void>(COMMANDS.environmentsDelete, { id: envId }),
      () => set(snapshot),
    );
  },
  setEnvironmentActive: async (envId) => {
    const prev = get().environments;
    set((s) => {
      const target = s.environments.find((env) => env.id === envId);
      if (!target) return {};
      return {
        activeEnvironmentId: envId,
        environments: s.environments.map((env) => {
          if (env.projectId !== target.projectId) return env;
          return { ...env, isActive: env.id === envId };
        }),
      };
    });
    await invokeWithRollback(
      'environments_set_active',
      () => invokeCmd<Environment>(COMMANDS.environmentsSetActive, { envId }),
      () => set({ environments: prev }),
    );
  },
});
