import { Package, FolderOpen, FolderClosed, Hash, Settings2 } from 'lucide-react';
import { useAppStore, type OpenTab, type TabKind } from '@/stores/app-store';
import { MethodPill } from '@/components/ui/method-pill';
import { HUE } from '@/lib/hue';
export const KIND_LABEL: Record<TabKind, string> = {
  project: '项目',
  module: '模块',
  collection: '集合',
  endpoint: '接口',
  // 草稿接口在 `nodes` 里没有行（保存前只是渲染状态），故若它落到
  // `missingDescriptor`，唯一诚实的标签是"未保存"而不是"已删除"。
  'endpoint-draft': '未保存',
  settings: '设置',
};
export interface TabDescriptor {
  tab: OpenTab;
  title: string;
  hint?: string;
  icon: React.ReactNode;
  /**
   * 类型名（作为一个词），只给 hover 标题用 —— 绝不上条：160px 放不下第二个
   * 词，图标已经能区分类型。
   *
   * 只给三种节点类型设。接口标签带 method pill，比"接口"二字信息更多；`settings`
   * 不需要（标题本身就是标签）。节点类型正是那些标题被去掉后右栏不再重复身份的
   * 类型 —— `EntitySettings` 现在没有自己的类型标签，这个词就住在这里。
   */
  kindLabel?: string;
  /**
   * 前导图标的文字色 class，让条保持可扫读，并与树中同一实体的那一行同色。
   *
   * 必须是 class 而不是手写 hex：hex 会与树自己的颜色漂开，且定死的 hex 无法
   * 跟随主题 —— 暗色下模块图标会停在 amber-400 而树行早已偏移。
   */
  iconColor: string;
}
// ----------------------------------------------------------------
//  标签描述符 —— 由原始 openTabs + 实体查表构建。`useTabDescriptors` 只依赖
//  openTabs 的数组引用与各实体数组，任一变化即重跑。
// ----------------------------------------------------------------

export function useTabDescriptors(tabs: OpenTab[]): TabDescriptor[] {
  const projects = useAppStore((s) => s.projects);
  // 模块 / 集合 / 接口共用同一个节点数组；detail map 只为接口的 method pill。
  const nodes = useAppStore((s) => s.nodes);
  const endpointDetails = useAppStore((s) => s.endpointDetails);
  // 未保存接口只住在这里，永远不进 `nodes`。没有它，条会把它们渲染成"已删除"
  // 兜底 —— 而那恰恰是草稿确定不是的东西。
  const draftEndpoints = useAppStore((s) => s.draftEndpoints);

  // 手写循环而非 .map().filter()，实体缺失时形状保持稳定（回落到"已删除"
  // 标签而不是什么都不渲染 —— `pruneStaleTabs` 下一拍会清掉它）。
  const out: TabDescriptor[] = [];
  for (const t of tabs) {
    if (t.kind === 'settings') {
      // 设置标签是单例 —— 没有底层实体可查。标题固定；分区选择住在面板内部，
      // 不在标签条上呈现。
      out.push({
        tab: t,
        title: '设置',
        icon: <Settings2 className="h-3.5 w-3.5" />,
        iconColor: 'text-muted-foreground',
      });
      continue;
    }
    if (t.kind === 'project') {
      const p = projects.find((x) => x.id === t.id);
      if (!p) {
        out.push(missingDescriptor(t));
        continue;
      }
      out.push({
        tab: t,
        title: p.name,
        hint: p.description || undefined,
        kindLabel: KIND_LABEL.project,
        icon: <Package className="h-3.5 w-3.5" />,
        iconColor: HUE.project.text,
      });
      continue;
    }
    if (t.kind === 'module') {
      const m = nodes.find((x) => x.id === t.id);
      if (!m) {
        out.push(missingDescriptor(t));
        continue;
      }
      out.push({
        tab: t,
        title: m.name,
        hint: m.description || undefined,
        kindLabel: KIND_LABEL.module,
        icon: m.expanded ? (
          <FolderOpen className="h-3.5 w-3.5" />
        ) : (
          <FolderClosed className="h-3.5 w-3.5" />
        ),
        iconColor: HUE.module.text,
      });
      continue;
    }
    if (t.kind === 'collection') {
      const c = nodes.find((x) => x.id === t.id);
      if (!c) {
        out.push(missingDescriptor(t));
        continue;
      }
      out.push({
        tab: t,
        title: c.name,
        hint: c.description || undefined,
        kindLabel: KIND_LABEL.collection,
        icon: c.expanded ? (
          <FolderOpen className="h-3.5 w-3.5" />
        ) : (
          <FolderClosed className="h-3.5 w-3.5" />
        ),
        iconColor: HUE.collection.text,
      });
      continue;
    }
    // endpoint-draft —— 与已保存接口同一套 pill 处理，就标签条而言它就是同一个
    // 对象，唯一差别是标题还在输入中。
    if (t.kind === 'endpoint-draft') {
      const d = draftEndpoints[t.id];
      if (!d) {
        out.push(missingDescriptor(t));
        continue;
      }
      out.push({
        tab: t,
        title: d.name,
        icon: (
          <MethodPill
            method={d.method}
            size="xs"
            variant="solid"
            title={`HTTP ${d.method}`}
            className="!h-3.5 !w-3.5 !text-[9px]"
          />
        ),
        iconColor: 'text-muted-foreground',
      });
      continue;
    }
    // endpoint —— method pill 来自 detail 行而非节点，所以配置缺失的标签会回落到
    // "已删除" 描述符，而不是渲染一个空 pill。
    const e = nodes.find((x) => x.id === t.id);
    const eDetail = e ? endpointDetails[e.id] : undefined;
    if (!e || !eDetail) {
      out.push(missingDescriptor(t));
      continue;
    }
    out.push({
      tab: t,
      title: e.name,
      icon: (
        <MethodPill
          method={eDetail.method}
          size="xs"
          variant="solid"
          title={`HTTP ${eDetail.method}`}
          className="!h-3.5 !w-3.5 !text-[9px]"
        />
      ),
      iconColor: 'text-muted-foreground',
    });
  }
  return out;
}

function missingDescriptor(tab: OpenTab): TabDescriptor {
  return {
    tab,
    // 草稿走到这里意味着它的文档在标签还开着时被丢弃了。"已删除"是错的 ——
    // 从来没有保存过 —— 而标签是用户唯一拿到的信号，必须说出实际发生了什么。
    title: tab.kind === 'endpoint-draft' ? '（草稿已丢弃）' : `（${KIND_LABEL[tab.kind]}已删除）`,
    icon: <Hash className="h-3.5 w-3.5" />,
    iconColor: 'text-destructive',
  };
}
