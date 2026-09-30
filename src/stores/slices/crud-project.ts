/**
 * CRUD sub-factory — projects.
 *
 * One of five sub-factories composed by `createCrudSlice` (see
 * `../crud-slice.ts` for the slice-wide contract: optimistic `set()` →
 * `invokeCmd` → snapshot-and-rollback).
 *
 * `deleteProject` is the widest cross-slice writer in the slice. It
 * removes the project's entire subtree, its environments, the response
 * history / saved examples keyed by the dropped endpoints, and the
 * matching workspace tabs — all in a single atomic `set()`. That
 * single-write shape is deliberate: rewriting it as a sequence of
 * `get().closeTab()`-style calls would split it into several renders and
 * make the IPC-failure `set(snapshot)` restore a state the user had
 * already moved past.
 */
import type { StateCreator } from 'zustand';
import type { Project } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { AppState } from '../app-store';
import { filterTabsAfterRemoval, invokeWithRollback, tabKeysForSubtree } from './crud-helpers';

/** Project actions owned by the CRUD slice. */
export interface ProjectCrudActions {
  addProject: (
    name: string,
    opts?: { description?: string; baseUrl?: string; color?: string },
  ) => Promise<string | null>;
  updateProject: (id: string, patch: Partial<Omit<Project, 'id' | 'createdAt'>>) => Promise<void>;
  deleteProject: (id: string) => Promise<void>;
}

export const createProjectCrud: StateCreator<AppState, [], [], ProjectCrudActions> = (
  set,
  get,
) => ({
  addProject: async (name, opts) => {
    try {
      const project = await invokeCmd<Project>(COMMANDS.projectsCreate, {
        name,
        opts: opts ?? null,
      });
      set((s) => ({ projects: [...s.projects, project] }));
      return project.id;
    } catch (e) {
      console.error('[store] projects_create failed', e);
      return null;
    }
  },
  updateProject: async (id, patch) => {
    // Optimistic local update + background invoke; rollback on failure.
    const prev = get().projects.find((p) => p.id === id);
    if (!prev) return;
    const optimistic: Project = { ...prev, ...patch, updatedAt: Date.now() };
    set((s) => ({
      projects: s.projects.map((p) => (p.id === id ? optimistic : p)),
    }));
    await invokeWithRollback(
      'projects_update',
      () => invokeCmd<Project>(COMMANDS.projectsUpdate, { id, patch }),
      () =>
        set((s) => ({
          projects: s.projects.map((p) => (p.id === id ? prev : p)),
        })),
    );
  },
  deleteProject: async (id) => {
    // Snapshot for rollback.
    const snapshot = {
      projects: get().projects,
      nodes: get().nodes,
      endpointDetails: get().endpointDetails,
      environments: get().environments,
      responseHistory: get().responseHistory,
      responseExamples: get().responseExamples,
      openTabs: get().openTabs,
      activeTabId: get().activeTabId,
      activeProjectId: get().activeProjectId,
      activeEndpointId: get().activeEndpointId,
    };
    set((s) => {
      // `rootId` is denormalised onto every node precisely so "what
      // belongs to this project" is one filter rather than a walk down
      // module → collection → endpoint.
      const removed = new Set(s.nodes.filter((n) => n.rootId === id).map((n) => n.id));
      const droppedEndpointIds = s.nodes
        .filter((n) => n.kind === 'endpoint' && removed.has(n.id))
        .map((n) => n.id);
      const nextHistory = { ...s.responseHistory };
      const nextExamples = { ...s.responseExamples };
      for (const eid of droppedEndpointIds) {
        delete nextHistory[eid];
        delete nextExamples[eid];
      }
      const nextDetails = { ...s.endpointDetails };
      for (const eid of droppedEndpointIds) delete nextDetails[eid];
      const tabs = filterTabsAfterRemoval(
        s.openTabs,
        s.activeTabId,
        tabKeysForSubtree(s.nodes, removed, 'project', id),
      );
      const nextActiveEndpointId =
        s.activeEndpointId && removed.has(s.activeEndpointId) ? null : s.activeEndpointId;
      return {
        projects: s.projects.filter((p) => p.id !== id),
        nodes: s.nodes.filter((n) => !removed.has(n.id)),
        endpointDetails: nextDetails,
        environments: s.environments.filter((env) => env.projectId !== id),
        responseHistory: nextHistory,
        responseExamples: nextExamples,
        activeProjectId:
          s.activeProjectId === id
            ? (s.projects.find((p) => p.id !== id)?.id ?? s.activeProjectId)
            : s.activeProjectId,
        activeEndpointId: nextActiveEndpointId,
        openTabs: tabs.tabs,
        activeTabId: tabs.activeId,
      };
    });
    await invokeWithRollback(
      'projects_delete',
      () => invokeCmd<void>(COMMANDS.projectsDelete, { id }),
      () => set(snapshot),
    );
  },
});
