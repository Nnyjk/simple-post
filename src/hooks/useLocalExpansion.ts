import { useCallback, useState } from 'react';

export interface LocalExpansion {
  isExpanded: (id: string) => boolean;
  toggle: (id: string) => void;
  expand: (id: string) => void;
  collapse: (id: string) => void;
}

/**
 * Local-only expand/collapse state for sub-list rows.
 *
 * Why local and not the persisted `entity.expanded` field? The
 * EntitySettings panels share the same entity objects as the left
 * tree, so mutating `module.expanded` / `collection.expanded` would
 * drag the tree along — collapsing a row in the right pane would
 * silently collapse it on the left, which is jarring. A per-pane
 * `Set<string>` keeps the right pane's expand/collapse state private.
 *
 * Returns the bare API; render-side filtering of sub-rows happens in
 * the consumer (e.g. `if (exp.isExpanded(id)) …`).
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