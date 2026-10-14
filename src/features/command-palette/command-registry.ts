/**
 * 命令面板（Ctrl/Cmd + K）的命令注册表。
 *
 * 单一工厂 `buildCommands()` 返回扁平 `Command` 列表 —— 既有静态动作（打开设置、切主题…），
 * 也有由状态派生的导航项（跳到接口、切环境…）；面板再对合并后的列表做一次模糊匹配。
 *
 * 约定：
 *  - `id` 全表唯一（既作 React key 也作去重标记）。
 *  - `title` 是主标签，`hint` 是右侧副标签。
 *  - `run` 在用户激活该行时调用；需要最新状态就读 `useAppStore.getState()`，
 *    不要捕获快照。
 *  - `section` 只用于结果列表的分组标题（"导航" / "命令"），不影响匹配。
 */

import { useAppStore } from '@/stores/app-store';
import type { EndpointDetail, Project, ResourceNode } from '@/types/domain';

export type CommandSection = '导航' | '命令';

export interface Command {
  id: string;
  title: string;
  hint?: string;
  section: CommandSection;
  /** 追加进搜索范围的关键词（已转小写）。 */
  keywords?: string[];
  run: () => void;
}

/** `buildCommands` 需要的快照形状。 */
interface RegistryState {
  projects: Project[];
  activeProjectId: string;
  activeEnvironmentId: string;
  environments: { id: string; projectId: string; name: string }[];
  /** 整棵树，打平；三种节点类型同在一个数组里。 */
  nodes: ResourceNode[];
  /** 接口的请求配置，按 node id 索引。 */
  endpointDetails: Record<string, EndpointDetail>;
}

/** 接口所属项目 —— 每行节点都带着反规范化的 `rootId`，读字段即可。 */
function endpointProjectId(endpointId: string, s: RegistryState): string | null {
  return s.nodes.find((n) => n.id === endpointId)?.rootId ?? null;
}

/** 生成完整命令列表。面板每次打开调一次即可（几十条，很便宜）。 */
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
      // 用 `openProject` 而非 `setActiveProject`：从面板选项目与欢迎页选择器、顶栏
      // 是同一个动作，三处必须落到同一状态（该项目的标签激活、树按它收窄）。
      run: () => useAppStore.getState().openProject(p.id),
    });
  }

  // ---------- 导航：跳到接口 ----------
  for (const ep of s.nodes) {
    if (ep.kind !== 'endpoint') continue;
    const detail = s.endpointDetails[ep.id];
    if (!detail) continue;
    const pid = endpointProjectId(ep.id, s);
    if (!pid) continue;
    const project = s.projects.find((p) => p.id === pid);
    cmds.push({
      id: `nav:endpoint:${ep.id}`,
      title: ep.name,
      hint: `${detail.method} · ${project?.name ?? ''}`,
      section: '导航',
      keywords: [detail.url, ...detail.tags, 'endpoint', '接口'],
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
    const firstMod = s.nodes.find((n) => n.kind === 'module' && n.rootId === s.activeProjectId);
    cmds.push({
      id: 'cmd:new-collection',
      title: firstMod ? '新建集合' : '新建集合（需先有模块）',
      hint: 'New collection',
      section: '命令',
      keywords: ['new', 'create', 'add', 'collection', '集合', '新建'],
      run: () =>
        useAppStore
          .getState()
          .openNewItem(
            firstMod ? { type: 'collection', parentModuleId: firstMod.id } : { type: 'collection' },
          ),
    });
  }

  // ---------- 命令：新建接口 ----------
  {
    // 新建接口不询问任何信息，故面板只需给出一个父节点就能触发动作。取项目第一个集合
    // 作为归宿；项目还没有集合则回落到第一个模块 —— 模块本身也是合法父节点。
    const firstCol = s.nodes.find((n) => n.kind === 'collection' && n.rootId === s.activeProjectId);
    const firstMod = firstCol
      ? s.nodes.find((n) => n.id === firstCol.parentId)
      : s.nodes.find((n) => n.kind === 'module' && n.rootId === s.activeProjectId);
    const parent = firstCol ?? (firstMod?.kind === 'module' ? firstMod : undefined);
    cmds.push({
      id: 'cmd:new-endpoint',
      title: parent ? '新建接口' : '新建接口（需先有模块）',
      hint: 'New endpoint',
      section: '命令',
      keywords: ['new', 'create', 'add', 'endpoint', '接口', 'request', '新建'],
      run: () => {
        if (!parent) return;
        // 面板里没有树行，故无重命名目标；但接口标题在工作区可改，且会直接开好标签。
        void useAppStore.getState().createEndpointAt(parent.id);
      },
    });
  }

  // ---------- 命令：复制当前 URL ----------
  if (s.activeEndpointId) {
    const ep = s.nodes.find((e) => e.id === s.activeEndpointId);
    const epDetail = s.endpointDetails[s.activeEndpointId];
    if (ep && epDetail) {
      cmds.push({
        id: 'cmd:copy-current-url',
        title: '复制当前接口 URL',
        hint: epDetail.method,
        section: '命令',
        keywords: ['copy', 'url', '复制', epDetail.url, ep.name],
        run: () => {
          if (typeof navigator !== 'undefined' && navigator.clipboard) {
            void navigator.clipboard.writeText(epDetail.url);
          }
        },
      });
    }
  }

  return cmds;
}
