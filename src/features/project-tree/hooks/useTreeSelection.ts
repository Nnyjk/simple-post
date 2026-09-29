import { useState, type MouseEvent as ReactMouseEvent } from 'react';
import { useAppStore } from '@/stores/app-store';

/**
 * Click handlers that open the right-hand tab for a tree node, plus the
 * endpoint multi-select used by drag-and-drop. Owns no store selectors
 * that build new arrays/objects — all values below are raw store
 * references or `.find()` results (stable).
 */
export function useTreeSelection() {
  // ---------- store actions ----------
  const setActiveEndpoint = useAppStore((s) => s.setActiveEndpoint);
  const setActiveProject = useAppStore((s) => s.setActiveProject);
  const openTab = useAppStore((s) => s.openTab);
  const toggleNodeExpanded = useAppStore((s) => s.toggleNodeExpanded);

  // ---------- click handlers (open tab + activate) ----------
  const openProjectTab = (id: string) => {
    setActiveProject(id);
    openTab('project', id);
  };
  const openModuleTab = (id: string) => {
    openTab('module', id);
    // Toggle the row's expanded state on every click. Earlier this
    // only expanded (skipped the toggle when already open), which
    // meant clicking an expanded module to a it open in the right pane
    // also collapsed it on the tree — but the user couldn't tell why
    // "收缩" never worked from the row click. Unconditional toggle
    // makes the row behave like every other tree widget.
    toggleNodeExpanded(id);
  };
  const openCollectionTab = (id: string) => {
    openTab('collection', id);
    toggleNodeExpanded(id);
  };
  const openEndpointTab = (id: string) => {
    setActiveEndpoint(id);
  };

  /**
   * Multi-select for endpoints. Ctrl+click toggles membership, plain
   * click on an endpoint clears the selection and selects only that
   * one. We only multi-select endpoints — multi-selecting modules or
   * collections is overkill for v1 of the DnD work and the rules
   * don't ask for it.
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
    openProjectTab,
    openModuleTab,
    openCollectionTab,
    openEndpointTab,
    selectedEndpointIds,
    handleEndpointClick,
  };
}
