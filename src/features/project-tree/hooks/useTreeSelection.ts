import { useState, type MouseEvent as ReactMouseEvent } from 'react';
import { useAppStore } from '@/stores/app-store';

/**
 * 为树节点打开右栏标签的点击处理器，外加供拖放使用的接口多选。不持有会新建数组 /
 * 对象的 store selector —— 下面的取值都是原始 store 引用或 `.find()` 的结果（稳定）。
 *
 * 「打开工作区」与「展开 / 收起」是**两件事**，各有一个入口：行按钮调 `openModuleTab` /
 * `openCollectionTab`，展开箭头调 `toggleExpanded`。曾经两者绑在同一个点击上，点箭头
 * 会顺手打开工作区、点名字会顺手收起刚展开的分支。
 */
export function useTreeSelection() {
  // ---------- store actions ----------
  const setActiveEndpoint = useAppStore((s) => s.setActiveEndpoint);
  const openTab = useAppStore((s) => s.openTab);
  const toggleNodeExpanded = useAppStore((s) => s.toggleNodeExpanded);

  // ---------- click handlers (open tab + activate) ----------
  // `useTreeSelection` 里没有 `openProjectTab`：项目不是这棵树里的一行，故树内没有任何
  // 可点击进入项目的元素。进入项目是 store 里的 `openProject`，由欢迎页选择器、顶栏选择器、
  // 命令面板与面包屑调用。
  const openModuleTab = (id: string) => {
    openTab('module', id);
  };
  const openCollectionTab = (id: string) => {
    openTab('collection', id);
  };
  /**
   * 展开 / 收起 —— 与「打开工作区」分开的那一半。只有展开箭头调它。
   */
  const toggleExpanded = (id: string) => {
    toggleNodeExpanded(id);
  };
  const openEndpointTab = (id: string) => {
    setActiveEndpoint(id);
  };

  /**
   * 接口的多选。Ctrl+点击切换成员，普通点击清空选区并只选这一个。只对接口做多选 —— 对
   * 模块或集合做多选对拖放的 v1 而言是过度设计，规则也没要求。
   */
  const [selectedEndpointIds, setSelectedEndpointIds] = useState<Set<string>>(() => new Set());

  // ---------- click / selection handlers ----------
  const handleEndpointClick = (id: string, e: ReactMouseEvent) => {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      setSelectedEndpointIds((prev) => {
        const next = new Set(prev);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return next;
      });
      return;
    }
    setSelectedEndpointIds(new Set([id]));
    openEndpointTab(id);
  };

  return {
    openModuleTab,
    openCollectionTab,
    toggleExpanded,
    openEndpointTab,
    selectedEndpointIds,
    handleEndpointClick,
  };
}
