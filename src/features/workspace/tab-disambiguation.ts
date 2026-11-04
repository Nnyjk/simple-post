/**
 * 标签条上的同名消歧 —— 给接口标签算出它所在容器的路径。
 *
 * **为什么需要它。** 标签的身份是 id，标题是接口名，而接口名在项目里**不唯一**：不同模块、
 * 不同集合下可以有同名接口，标签条于是摆出两个一字不差的标签，用户无从分辨哪个是哪个。
 * 标签宽度是固定的 160px，放不下完整路径，故路径要挑着放。
 *
 * **为什么可见的提示只在重名时出现。** 提示占掉标题右侧约 40px，平时留着就是白花掉标题的
 * 空间。故只有这批标签里确实有同名接口时，才给它补一段可见提示；没有冲突时标签与原来
 * 一模一样。
 *
 * **提示只写直接父容器。** 可见的那一段是路径的最后一段（最靠近接口的容器），完整路径
 * 写在悬停提示里：40px 只放得下两三个汉字，而完整路径可能有四段（模块 › 集合 › 子集合）。
 */
import type { ResourceNode } from '@/types/domain';
import { containerPathOf } from '@/lib/tree-index';

export interface EndpointTabSource {
  /** 标签 id。 */
  id: string;
  /** 标签标题，即接口名。 */
  title: string;
  /** 接口所在容器的节点 id。已存接口与未保存的草稿都给得出。 */
  parentId: string | null;
}

export interface TabPathInfo {
  /** 完整路径，从模块往下逐层用「›」隔开。接口还没有容器时是空串。 */
  path: string;
  /** 标题右侧那一小段提示。重名时才是非空串，其余时候是空串。 */
  hint: string;
}

/**
 * 算出每个接口标签的路径，并标出哪些标签需要可见的消歧提示。
 *
 * 返回的 map 覆盖**每一个**接口标签：完整路径始终算得出（它不占地方，只写进悬停提示），
 * 而可见的那一段提示只在同名时才有。
 *
 * 只在**接口类**标签之间比名字：模块与集合有各自的图标，一个叫「用户列表」的模块与一个
 * 叫「用户列表」的接口不会让人认错。
 */
export function endpointTabPaths(
  sources: readonly EndpointTabSource[],
  nodes: readonly ResourceNode[],
): Map<string, TabPathInfo> {
  const out = new Map<string, TabPathInfo>();

  // 先按标题分组：同一组里的标签互相之间才需要区分。
  const byTitle = new Map<string, EndpointTabSource[]>();
  for (const s of sources) {
    const group = byTitle.get(s.title);
    if (group) group.push(s);
    else byTitle.set(s.title, [s]);
  }

  for (const group of byTitle.values()) {
    // 这个标题下只有一个标签时，用户不会认错，不需要可见提示。
    const collides = group.length > 1;
    for (const s of group) {
      const segments = containerPathOf(nodes, s.parentId);
      out.set(s.id, {
        path: segments.join(' › '),
        // 找不到容器（父节点还没建出来，或树还没加载完）：没有路径可写。
        hint: collides && segments.length > 0 ? segments[segments.length - 1] : '',
      });
    }
  }
  return out;
}
