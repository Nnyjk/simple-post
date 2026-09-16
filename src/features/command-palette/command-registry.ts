/**
 * Command registry for the Command Palette (Ctrl/Cmd + K).
 *
 * The registry exposes a single factory `buildCommands(state)` that returns a
 * flat list of `Command` items — both static actions (open settings, switch
 * theme, ...) and state-derived navigation entries (jump to endpoint, switch
 * active environment, ...). The palette then performs a single fuzzy match
 * pass over the combined list.
 *
 * Conventions:
 *  - `id` must be unique across the list (used as React key + de-dup marker).
 *  - `title` is the user-facing label; `hint` is a secondary line (right side).
 *  - `run` is invoked when the user activates the row. It may read latest state
 *    via `useAppStore.getState()` so we never capture a stale snapshot.
 *  - `section` is a coarse grouping ("导航", "命令") used only for the result
 *    list header — matching is unaffected.
 */

import { useAppStore } from '@/stores/app-store';
import type { Project } from '@/types/domain';

export type CommandSection = '导航' | '命令';

export interface Command {
  id: string;
  title: string;
  hint?: string;
  section: CommandSection;
  /** Optional keywords appended to the search haystack (lowercased). */
  keywords?: string[];
  run: () => void;
}

/** Snapshot shape needed by `buildCommands`. */
interface RegistryState {
  projects: Project[];
  activeProjectId: string;
  activeEnvironmentId: string;
  environments: { id: string; projectId: string; name: string }[];
  endpoints: { id: string; name: string; url: string; method: string; tags: string[]; collectionId: string }[];
  collections: { id: string; moduleId: string; name: string }[];
  modules: { id: string; projectId: string; name: string }[];
}

/** Pick a representative project for an endpoint (deepest ancestor). */
function endpointProjectId(
  endpointId: string,
  s: RegistryState,
): string | null {
  const ep = s.endpoints.find((e) => e.id === endpointId);
  if (!ep) return null;
  const col = s.collections.find((c) => c.id === ep.collectionId);
  if (!col) return null;
  const mod = s.modules.find((m) => m.id === col.moduleId);
  if (!mod) return null;
  return mod.projectId;
}

/** Build the full command list. Call once per palette open (cheap, ~tens of items). */
export function buildCommands(): Command[] {
  const s = useAppStore.getState();
  const cmds: Command[] = [];

  // ---------- 导航：跳到项目 ----------
  for (const p of s.projects) {
    cmds.push({
      id: `nav:project:${p.id}`,
      title: p.name,
      hint: '切换项目',
      section: '导航',
      keywords: ['project', '项目', p.description ?? ''],
      run: () => useAppStore.getState().setActiveProject(p.id),
    });
  }

  // ---------- 导航：跳到接口 ----------
  for (const ep of s.endpoints) {
    const pid = endpointProjectId(ep.id, s);
    if (!pid) continue;
    const project = s.projects.find((p) => p.id === pid);
    cmds.push({
      id: `nav:endpoint:${ep.id}`,
      title: ep.name,
      hint: `${ep.method} · ${project?.name ?? ''}`,
      section: '导航',
      keywords: [ep.url, ...ep.tags, 'endpoint', '接口'],
      run: () => {
        const store = useAppStore.getState();
        if (store.activeProjectId !== pid) store.setActiveProject(pid);
        store.setActiveEndpoint(ep.id);
      },
    });
  }

  // ---------- 命令：切换环境 ----------
  const envs = s.environments.filter((e) => e.projectId === s.activeProjectId);
  for (const env of envs) {
    cmds.push({
      id: `cmd:env:${env.id}`,
      title: `切换环境：${env.name}`,
      hint: 'Environment',
      section: '命令',
      keywords: ['env', 'environment', '环境', env.name],
      run: () => useAppStore.getState().setActiveEnvironment(env.id),
    });
  }

  // ---------- 命令：切换主题 ----------
  const themes: { id: 'dark' | 'light' | 'system'; label: string }[] = [
    { id: 'dark', label: '暗色' },
    { id: 'light', label: '亮色' },
    { id: 'system', label: '跟随系统' },
  ];
  for (const t of themes) {
    cmds.push({
      id: `cmd:theme:${t.id}`,
      title: `切换主题：${t.label}`,
      hint: s.theme === t.id ? '当前' : 'Theme',
      section: '命令',
      keywords: ['theme', '主题', t.id, t.label],
      run: () => useAppStore.getState().setTheme(t.id),
    });
  }

  // ---------- 命令：打开设置 ----------
  cmds.push({
    id: 'cmd:open-settings',
    title: '打开设置',
    hint: 'Settings',
    section: '命令',
    keywords: ['settings', 'preferences', '设置', '偏好'],
    run: () => useAppStore.getState().openSettingsTab(),
  });

  // ---------- 命令：新建项目 ----------
  cmds.push({
    id: 'cmd:new-project',
    title: '新建项目',
    hint: 'New project',
    section: '命令',
    keywords: ['new', 'create', 'add', 'project', '项目', '新建'],
    run: () => {
      useAppStore.getState().openNewItem({ type: 'project' });
    },
  });

  // ---------- 命令：新建模块 ----------
  cmds.push({
    id: 'cmd:new-module',
    title: '新建模块',
    hint: 'New module',
    section: '命令',
    keywords: ['new', 'create', 'add', 'module', '模块', '新建'],
    run: () => {
      useAppStore.getState().openNewItem({ type: 'module' });
    },
  });

  // ---------- 命令：新建集合 ----------
  {
    const firstMod = s.modules.find((m) => m.projectId === s.activeProjectId);
    cmds.push({
      id: 'cmd:new-collection',
      title: firstMod ? '新建集合' : '新建集合（需先有模块）',
      hint: 'New collection',
      section: '命令',
      keywords: ['new', 'create', 'add', 'collection', '集合', '新建'],
      run: () =>
        useAppStore.getState().openNewItem(
          firstMod ? { type: 'collection', parentModuleId: firstMod.id } : { type: 'collection' },
        ),
    });
  }

  // ---------- 命令：新建接口 ----------
  {
    const projectMods = s.modules.filter((m) => m.projectId === s.activeProjectId);
    const firstCol = s.collections.find((c) => projectMods.some((m) => m.id === c.moduleId));
    const firstMod = firstCol
      ? projectMods.find((m) => m.id === firstCol.moduleId)
      : projectMods[0];
    cmds.push({
      id: 'cmd:new-endpoint',
      title: firstCol ? '新建接口' : '新建接口（需先有集合）',
      hint: 'New endpoint',
      section: '命令',
      keywords: ['new', 'create', 'add', 'endpoint', '接口', 'request', '新建'],
      run: () =>
        useAppStore.getState().openNewItem(
          firstCol
            ? { type: 'endpoint', parentCollectionId: firstCol.id, parentModuleId: firstCol.moduleId }
            : firstMod
              ? { type: 'endpoint', parentModuleId: firstMod.id }
              : { type: 'endpoint' },
        ),
    });
  }

  // ---------- 命令：复制当前 URL ----------
  if (s.activeEndpointId) {
    const ep = s.endpoints.find((e) => e.id === s.activeEndpointId);
    if (ep) {
      cmds.push({
        id: 'cmd:copy-current-url',
        title: '复制当前接口 URL',
        hint: ep.method,
        section: '命令',
        keywords: ['copy', 'url', '复制', ep.url, ep.name],
        run: () => {
          if (typeof navigator !== 'undefined' && navigator.clipboard) {
            void navigator.clipboard.writeText(ep.url);
          }
        },
      });
    }
  }

  return cmds;
}
