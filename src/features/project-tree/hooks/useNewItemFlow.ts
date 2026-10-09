import { useEffect, useState } from 'react';
import { useAppStore, type NewType } from '@/stores/app-store';
import type { EditTarget } from '../tree-types';

/**
 * Bridges the global "new item" modal with the tree: records the intent
 * behind each `openNewItem()` call and, once the modal reports the
 * created row, drops that row straight into inline rename.
 */
export function useNewItemFlow(setEditing: (t: EditTarget) => void) {
  const openNewItem = useAppStore((s) => s.openNewItem);
  const lastCreatedId = useAppStore((s) => s.lastCreatedId);
  const clearLastCreated = useAppStore((s) => s.clearLastCreated);

  const [lastIntent, setLastIntent] = useState<NewType | null>(null);

  const openNewItemWithIntent = (opts: {
    type: NewType;
    parentModuleId?: string;
    parentCollectionId?: string;
  }) => {
    setLastIntent(opts.type);
    openNewItem(opts);
  };

  const handleWorkspaceNew = () => {
    setLastIntent(null);
    openNewItem();
  };

  useEffect(() => {
    if (!lastCreatedId) return;
    if (!lastIntent) return;
    // A project has no row here, so there is nothing to drop into inline
    // rename — the tree lists modules / collections / endpoints only.
    // `submitNewItem` already entered the new project, whose name the
    // user typed in the modal; entering rename mode would only leave a
    // stale `editing` target behind.
    if (lastIntent !== 'project') setEditing({ kind: lastIntent, id: lastCreatedId });
    setLastIntent(null);
    clearLastCreated();
  }, [lastCreatedId, lastIntent, clearLastCreated, setEditing]);

  return { openNewItemWithIntent, handleWorkspaceNew };
}
