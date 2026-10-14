import { useCallback, useState } from 'react';

export interface LocalExpansion {
  isExpanded: (id: string) => boolean;
  toggle: (id: string) => void;
  expand: (id: string) => void;
  collapse: (id: string) => void;
}

/**
 * 子列表行的展开状态，只存在本组件。
 *
 * 不写 `entity.expanded`：EntitySettings 面板与左侧树共用同一批 entity 对象，
 * 改它会让右栏折叠时左栏跟着折叠。
 */
export function useLocalExpansion(): LocalExpansion {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  const isExpanded = useCallback((id: string) => expanded.has(id), [expanded]);

  const toggle = useCallback((id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const expand = useCallback((id: string) => {
    setExpanded((prev) => {
      if (prev.has(id)) return prev;
      const next = new Set(prev);
      next.add(id);
      return next;
    });
  }, []);

  const collapse = useCallback((id: string) => {
    setExpanded((prev) => {
      if (!prev.has(id)) return prev;
      const next = new Set(prev);
      next.delete(id);
      return next;
    });
  }, []);

  return { isExpanded, toggle, expand, collapse };
}
