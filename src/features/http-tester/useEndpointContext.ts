/**
 * 「这个接口在树上处于什么位置」的解析结果。
 *
 * 从原始 store 数组派生，**不新建 store 侧的引用** —— 逐个 `useAppStore` 取用，保持
 * Zustand v5「selector 不新建对象」规则，派生列表一律在本地 `useMemo` 里算。
 *
 * **base URL 的解析不在这里**：它在 `model/endpointParts.ts` 的
 * `buildEndpointRequestParts` 里，与单次发送共用同一份推导。这里的 `moduleId` 与
 * `endpointEnv` 是那份推导的输入。
 */
import { useMemo } from 'react';
import { useAppStore, useActiveEnvironment } from '@/stores/app-store';
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
 * 解析接口的项目 / 祖先链 / 环境 / 模块 / baseUrl 选择器状态。
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
  selection: BaseUrlSelection;
  isCustomUrl: boolean;
} {
  // 原始 store slice —— 接口的 project / ancestors 在本地 useMemo 派生。
  // 逐个 `useAppStore` 取用，保持 Zustand v5「selector 不新建对象」规则。
  const allNodes = useAppStore((s) => s.nodes);
  const allProjects = useAppStore((s) => s.projects);
  // 工作区当前环境（顶栏的 ENV dev 药丸）。它是本客户端的实时状态，切一次全部接口
  // 一起换地址与端口。
  const workspaceEnv = useActiveEnvironment();

  // 接口所属项目，直接取反规范化的 `rootId`：单棵树上每行都带着这个答案。
  const project = useMemo(
    () => (endpoint ? allProjects.find((p) => p.id === endpoint.rootId) : undefined),
    [endpoint, allProjects],
  );
  // 页面顶部 `项目 › 模块 › 集合` 面包屑的祖先链，与设置面板（`设置 › 连接`）用同一套
  // 写法。沿 `parentId` 向上收集、根在前，故集合嵌套超过一层也无需额外分支。
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
  // 该接口 baseUrl 所属的环境 = 工作区**当前环境**（顶栏的 ENV 药丸）。
  //
  // 接口不再绑定环境：库里没有 `endpoint_details.environment_id` 那一列，界面上也没有
  // 「这个接口自己选了哪个环境」这回事。当前环境是每个客户端自己的实时状态，切一次全
  // 部接口一起换地址 —— 这正是「新建环境并切换后端口不跟着换」那个问题的正解。
  const endpointEnv = workspaceEnv;

  // 该接口所处模块。base URL 的**值**按模块区分，故读取那一格时需要它。
  //
  // `endpoint` 作为向上遍历的起始行传入而非按 id 查：草稿接口没有 `nodes` 行，只按
  // id 查会得到「无模块」而永远读不到该格（见 `moduleIdOfEndpoint` 的 `self`）。
  const moduleId = useMemo(
    () => moduleIdOfEndpoint(allNodes, endpoint?.id, endpoint),
    [allNodes, endpoint],
  );

  // 驱动选择器：反映接口上现存的选择。用户改选时 `commitBaseUrl` 写到这个接口存放
  // baseUrl 选择的位置 —— 已保存的写该行，未保存的写草稿文档。已保存的立即落库
  // （此选择没有 Save 按钮），草稿则带到 Save 时并入该行。
  //
  // `baseUrlOverride` 是三态，空串正是自定义的意义：`null` = 非自定义模式，
  // `''` = 自定义模式、URL 框承载完整地址。按真值判断会把两者混同：点自定义写入
  // `''` 后下次渲染读成 falsy，选择器弹回 跟随模块，点击看起来毫无效果。
  const selection: BaseUrlSelection = useMemo(() => {
    if (!detail) return { kind: 'module' };
    if (detail.baseUrlOverride !== null && detail.baseUrlOverride !== undefined) {
      return { kind: 'override' };
    }
    return { kind: 'module' };
  }, [detail]);
  // 自定义 让 URL 框承载完整地址，故不得再给它加任何前缀。`resolved.baseUrl` 也
  // 收敛为空串（拼接时 path 原样不变，见 joinBaseUrl），但这里读 selection，好让仍
  // 带 host 的历史 override 不掩盖这个意图。
  const isCustomUrl = selection.kind === 'override';

  return { project, ancestors, endpointEnv, moduleId, selection, isCustomUrl };
}
