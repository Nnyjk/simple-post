import { useCallback, useEffect, useState } from 'react';
import { useAppStore, type NewDraftKind } from '@/stores/app-store';
import type { EditTarget } from '../tree-types';

/**
 * Bridges the create dialogs with the tree: records the intent behind
 * each `openNewItem()` call and, once the modal reports the created row,
 * drops that row straight into inline rename.
 *
 * There is no project row here to drop into, so a created *project* skips
 * the rename step — `submitNewItem` already entered it, and its name was
 * typed in the modal.
 *
 * Endpoints do not pass through this hook at all: `createEndpointAt`
 * returns the id it just created, so the caller renames that row directly
 * instead of round-tripping through `lastCreatedId`. Sharing the signal
 * would have meant two sources of truth for "the row I just made".
 */
export function useNewItemFlow(setEditing: (t: EditTarget) => void) {
  const openNewItem = useAppStore((s) => s.openNewItem);
  const createEndpointAt = useAppStore((s) => s.createEndpointAt);
  const lastCreatedId = useAppStore((s) => s.lastCreatedId);
  const clearLastCreated = useAppStore((s) => s.clearLastCreated);

  const [lastIntent, setLastIntent] = useState<NewDraftKind | null>(null);

  const openNewItemWithIntent = useCallback(
    (opts: { type: NewDraftKind; parentModuleId?: string; parentCollectionId?: string }) => {
      setLastIntent(opts.type);
      openNewItem(opts);
    },
    [openNewItem],
  );

  /**
   * Create an endpoint under `parentId` and open it in the workspace.
   *
   * There is deliberately no inline rename here any more. Creating an
   * endpoint no longer writes a row: `createEndpointAt` returns a
   * `draft:`-prefixed id for an endpoint that lives in the renderer until
   * Save (see `stores/slices/ui-draft-endpoints.ts`). There is no tree
   * row to drop into rename, and `setEditing({ kind: 'endpoint', id })`
   * would aim the editor at an id no row has — the tree would sit with a
   * pending rename target for a row that does not exist.
   *
   * The naming step moved along with the row. An unnamed draft opens its
   * workspace title in edit mode instead, which is where the name is
   * actually read before the endpoint is saved.
   */
  const createEndpoint = useCallback(
    async (parentId: string) => {
      await createEndpointAt(parentId);
    },
    [createEndpointAt],
  );

  useEffect(() => {
    if (!lastCreatedId) return;
    if (!lastIntent) return;
    // A project has no row here, so there is nothing to drop into inline
    // rename — the tree lists modules / collections / endpoints only, and
    // `submitNewItem` already entered the new project. Entering rename
    // would only leave a stale `editing` target behind.
    if (lastIntent !== 'project') setEditing({ kind: lastIntent, id: lastCreatedId });
    setLastIntent(null);
    clearLastCreated();
  }, [lastCreatedId, lastIntent, clearLastCreated, setEditing]);

  return { openNewItemWithIntent, createEndpoint };
}
