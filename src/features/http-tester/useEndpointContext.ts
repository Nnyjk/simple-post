/**
 * 「这个接口在树上处于什么位置」的解析结果。
 *
 * 从原始 store 数组派生，**不新建 store 侧的引用** —— 逐个 `useAppStore` 取用，保持
 * Zustand v5「selector 不新建对象」规则，派生列表一律在本地 `useMemo` 里算。
 */
import { useMemo } from 'react';
import { useAppStore, useActiveEnvironment } from '@/stores/app-store';
import { resolveEndpointBaseUrl } from '@/lib/url';
import { moduleIdOfEndpoint } from '@/lib/tree-index';
import type { BaseUrlSelection } from './BaseUrlPicker';
import type { EndpointDetail, Environment, Project, ResourceNode } from '@/types/domain';

/** 面包屑用的一节祖先。`type` 取自 `ResourceNode.kind`，此处按字符串透传。 */
export interface Ancestor {
  id: string;
  name: string;
  type: string;
}

/**
 * 解析接口的项目 / 祖先链 / 环境 / 模块 / 有效 baseUrl / 选择器状态。
 *
 * 这些值只依赖 store 里的数据，故合成一个 hook；`HttpTester` 读它们渲染面包屑、
 * 选择器与 URL 栏，并把 `commitBaseUrl` 之外的写操作留在自己那里。
 */
export function useEndpointContext(
  endpoint: ResourceNode | null,
  detail: EndpointDetail | undefined,
): {
  project: Project | undefined;
  ancestors: Ancestor[];
  endpointEnv: Environment | undefined;
  moduleId: ReturnType<typeof moduleIdOfEndpoint>;
  resolved: ReturnType<typeof resolveEndpointBaseUrl>;
  selection: BaseUrlSelection;
  isCustomUrl: boolean;
} {
  // 原始 store slice —— 接口的 project / ancestors 在本地 useMemo 派生。
  // 逐个 `useAppStore` 取用，保持 Zustand v5「selector 不新建对象」规则。
  const allNodes = useAppStore((s) => s.nodes);
  const allProjects = useAppStore((s) => s.projects);
  const allEnvironments = useAppStore((s) => s.environments);
  // 工作区当前环境（顶栏的 ENV dev 药丸）。接口未自选环境时作兜底，让新建接口的
  // baseUrl + 变量解析直接可用，不必先走一遍选择器。
  const workspaceEnv = useActiveEnvironment();

  // 接口所属项目，直接取反规范化的 `rootId`：单棵树上每行都带着这个答案。
  const project = useMemo(
    () => (endpoint ? allProjects.find((p) => p.id === endpoint.rootId) : undefined),
    [endpoint, allProjects],
  );
  // 页面顶部 `项目 › 模块 › 集合` 面包屑的祖先链，与设置面板（`设置 › 连接`）同一套
  // 锚定写法。沿 `parentId` 向上收集、根在前，故集合嵌套超过一层也无需额外分支。
  const ancestors = useMemo(() => {
    if (!endpoint) return [];
    const chain: Ancestor[] = [];
    const seen = new Set<string>([endpoint.id]);
    let cur = endpoint.parentId ? allNodes.find((n) => n.id === endpoint!.parentId) : undefined;
    while (cur && !seen.has(cur.id)) {
      seen.add(cur.id);
      chain.unshift({ id: cur.id, name: cur.name, type: cur.kind });
      cur = cur.parentId ? allNodes.find((n) => n.id === cur!.parentId) : undefined;
    }
    return chain;
  }, [endpoint, allNodes]);
  // 该接口 baseUrl 所属的环境：优先 detail 的 `environmentId`，否则回落到工作区当前
  // 环境（顶栏 ENV dev 药丸），免得新建接口因为还没指定环境就没有 baseUrl。
  const endpointEnv = useMemo(() => {
    if (detail?.environmentId) {
      return allEnvironments.find((e) => e.id === detail.environmentId);
    }
    return workspaceEnv;
  }, [detail, allEnvironments, workspaceEnv]);

  // 该接口的有效 baseUrl —— 同时驱动选择器标签与发送时的拼接。
  //
  // 服务列表属于项目，故选择器给出全部；按模块区分的是读 URL 的**单元格**。
  // `moduleId` 仍必需：它挑出本接口读的那一行 `env.moduleBaseUrls[defId]`，但不再
  // 收窄可选服务。
  //
  // `endpoint` 作为向上遍历的起始行传入而非按 id 查：草稿接口没有 `nodes` 行，只按
  // id 查会得到「无模块」而永远读不到该单元格（见 `moduleIdOfEndpoint` 的 `self`）。
  const moduleId = useMemo(
    () => moduleIdOfEndpoint(allNodes, endpoint?.id, endpoint),
    [allNodes, endpoint],
  );
  const resolved = useMemo(
    () =>
      resolveEndpointBaseUrl(
        detail ?? { environmentId: null, baseUrlDefinitionId: null, baseUrlOverride: null },
        endpointEnv,
        project,
        moduleId,
      ),
    [detail, endpointEnv, project, moduleId],
  );

  // 驱动选择器：反映接口上现存的选择。用户改选时 `commitBaseUrl` 写到这个接口存放
  // baseUrl 选择的位置 —— 已保存的写该行，未保存的写草稿文档。已保存的立即落库
  // （此选择没有 Save 按钮），草稿则带到 Save 时并入该行。
  //
  // `baseUrlOverride` 是三态，空串正是自定义的意义：`null` = 非自定义模式，
  // `''` = 自定义模式、URL 框承载完整地址。按真值判断会把两者混同：点自定义写入
  // `''` 后下次渲染读成 falsy，选择器弹回 项目默认，点击看起来毫无效果。
  const selection: BaseUrlSelection = useMemo(() => {
    if (!detail) return { kind: 'default' };
    if (detail.baseUrlOverride !== null && detail.baseUrlOverride !== undefined) {
      return { kind: 'override' };
    }
    if (detail.baseUrlDefinitionId) {
      return { kind: 'definition', definitionId: detail.baseUrlDefinitionId };
    }
    return { kind: 'default' };
  }, [detail]);
  // 自定义 让 URL 框承载完整地址，故不得再给它加任何前缀。`resolved.baseUrl` 也
  // 收敛为空串（拼接时 path 原样不变，见 joinBaseUrl），但这里读 selection，好让仍
  // 带 host 的历史 override 不掩盖这个意图。
  const isCustomUrl = selection.kind === 'override';

  return { project, ancestors, endpointEnv, moduleId, resolved, selection, isCustomUrl };
}
