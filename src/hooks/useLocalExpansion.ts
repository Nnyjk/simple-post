import { useCallback, useState } from 'react';

export interface LocalExpansion {
  /** 这一行是不是展开着。 */
  isExpanded: (id: string) => boolean;
  /** 展开这一行；它已经展开着就收起。 */
  toggle: (id: string) => void;
}

/**
 * 一组「行」的展开状态，状态只活在使用它的那个组件里。
 *
 * 不写进 store：左侧树的展开状态由 `bootstrap-slice` 里的 `nodes[].expanded` 持有，
 * 因为整份替换 `nodes` 时要把用户打开的分支合回去。面板里的行不在那棵树里，
 * 把它们放进同一处只会让两边的展开互相牵动。
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

  return { isExpanded, toggle };
}
