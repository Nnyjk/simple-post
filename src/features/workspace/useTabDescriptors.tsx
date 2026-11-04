import { Package, FolderOpen, FolderClosed, Hash, Settings2, Braces } from 'lucide-react';
import { useAppStore, type OpenTab, type TabKind } from '@/stores/app-store';
import { MethodPill } from '@/components/ui/method-pill';
import { HUE } from '@/lib/hue';
import { endpointTabPaths, type EndpointTabSource } from './tab-disambiguation';
export const KIND_LABEL: Record<TabKind, string> = {
  project: '项目',
  module: '模块',
  collection: '集合',
  endpoint: '接口',
  // 草稿接口在 `nodes` 里没有行（保存前只是渲染状态），故若它落到
  // `missingDescriptor`，唯一诚实的标签是"未保存"而不是"已删除"。
  'endpoint-draft': '未保存',
  settings: '设置',
  'dynamic-variables': '动态变量',
};
export interface TabDescriptor {
  tab: OpenTab;
  title: string;
  hint?: string;
  /**
   * 接口所在容器的完整路径，从模块往下。它只写进悬停提示，不占标签的宽度，故与 `hint`
   * 分开：`hint` 是标题右侧那一小段可见文字，`path` 是它的完整版。
   */
  path?: string;
  icon: React.ReactNode;
  /**
   * 类型名（作为一个词），只给 hover 标题用 —— 绝不上条：160px 放不下第二个词，图标已经能
   * 区分类型。
   *
   * 只给三种节点类型设。接口标签带 method pill，比「接口」二字信息更多。
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
  // 的样子 —— 而那恰恰是草稿确定不是的东西。
  const draftEndpoints = useAppStore((s) => s.draftEndpoints);

  // 接口标签要不要显示路径提示，取决于**这批标签里**有没有同名接口，故先扫一遍接口类标签
  // （已存接口与未保存的草稿），算出它们的标题与所在容器。
  const endpointSources: EndpointTabSource[] = [];
  for (const t of tabs) {
    if (t.kind === 'endpoint') {
      const e = nodes.find((x) => x.id === t.id);
      if (e) endpointSources.push({ id: t.id, title: e.name, parentId: e.parentId });
    } else if (t.kind === 'endpoint-draft') {
      const d = draftEndpoints[t.id];
      if (d) endpointSources.push({ id: t.id, title: d.name, parentId: d.parentId });
    }
  }
  const endpointPaths = endpointTabPaths(endpointSources, nodes);

  // 手写循环而非 .map().filter()，实体缺失时形状保持稳定（回落到"已删除"
  // 标签而不是什么都不渲染 —— `pruneStaleTabs` 下一拍会清掉它）。
  const out: TabDescriptor[] = [];
  for (const t of tabs) {
    if (t.kind === 'settings') {
      // 设置标签是单例 —— 没有底层实体可查。标题固定；分区选择住在面板内部，
      // 标签条不呈现它。
      out.push({
        tab: t,
        title: '设置',
        icon: <Settings2 className="h-3.5 w-3.5" />,
        iconColor: 'text-muted-foreground',
      });
      continue;
    }
    if (t.kind === 'dynamic-variables') {
      // 动态变量是项目内的一页：标签 id 里带着项目 id，而标题固定 —— 标题要说的是
      // 「这是哪一页」，项目名由页面自己（顶栏那一侧）说。
      out.push({
        tab: t,
        title: '动态变量',
        icon: <Braces className="h-3.5 w-3.5" />,
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
    // endpoint-draft 标签与已保存接口共用同一套 pill 处理；就标签条而言它是同一个
    // 对象，唯一差别是标题还在输入中。
    if (t.kind === 'endpoint-draft') {
      const d = draftEndpoints[t.id];
      if (!d) {
        out.push(missingDescriptor(t));
        continue;
      }
      const info = endpointPaths.get(t.id);
      out.push({
        tab: t,
        title: d.name,
        hint: info?.hint || undefined,
        path: info?.path || undefined,
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
    const info = endpointPaths.get(t.id);
    out.push({
      tab: t,
      title: e.name,
      hint: info?.hint || undefined,
      path: info?.path || undefined,
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
    // 草稿走到 `missingDescriptor` 意味着它的文档在标签还开着时被丢弃了。"已删除"是错的 ——
    // 从来没有保存过 —— 而标签是用户唯一拿到的信号，必须说出实际发生了什么。
    title: tab.kind === 'endpoint-draft' ? '（草稿已丢弃）' : `（${KIND_LABEL[tab.kind]}已删除）`,
    icon: <Hash className="h-3.5 w-3.5" />,
    iconColor: 'text-destructive',
  };
}
