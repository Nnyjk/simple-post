/**
 * WorkspaceShell — the application frame for when a project is active.
 *
 * WHY THIS IS A SEPARATE MODULE
 * -----------------------------
 * `App.tsx` switches between two surfaces on `activeProjectId === ''`:
 * the standalone welcome screen, and this. Keeping the workspace in its
 * own module is what lets `App.tsx` `lazy()` it, so opening the app
 * fetches the store and the picker only. The tree, the top bar, the HTTP
 * tester, the settings panels and the CodeMirror editors are not fetched
 * until the user actually enters a project.
 *
 * That pairs with the two-layer `bootstrap()` in `bootstrap-slice.ts`:
 * layer 1 (`projects`) unblocks the splash, layer 2 (tree / details /
 * environments) streams in behind it.
 *
 * WHAT IS DELIBERATELY NOT MOUNTED HERE
 * --------------------------------------
 * `ProjectTree` only renders when a project is active. With no project it
 * had nothing to show but a "尚未选择项目" placeholder column — a dead
 * 256px panel plus a search box, a 新建 button that opens a modal which
 * cannot succeed without a parent project, and a footer count of zero.
 *
 * The settings tab used to be reachable from here with no project open,
 * which is why this shell once had to render the settings panel without
 * a tree. It no longer does: `App.tsx` owns the project-less settings
 * surface (`features/settings/SettingsSurface.tsx`), so reaching the
 * same panel body from the welcome screen costs this shell nothing. Both
 * hosts mount the one `SettingsPanel`; only the chrome differs.
 */

import { Suspense, lazy, useEffect, useRef, useState } from 'react';
import { TopBar } from '@/features/top-bar/TopBar';
import { ProjectTree } from '@/features/project-tree/ProjectTree';
import { HttpTester } from '@/features/http-tester/HttpTester';
import { WorkspaceTabs } from '@/features/workspace/WorkspaceTabs';
import { Splitter } from '@/components/ui/splitter';
import { useAppStore } from '@/stores/app-store';

// ---------------------------------------------------------------------------
// Code splitting — see the note above each lazy boundary.
//
// These are all panels that only exist *inside* this shell, so making
// them async costs nothing at first paint (the shell itself is already
// an async chunk) and keeps them out of each other's chunks.
// ---------------------------------------------------------------------------

// CodeMirror (425 kB raw / 139 kB gzip). Only an endpoint tab needs it.
const ResponseViewer = lazy(() =>
  import('@/features/response-viewer/ResponseViewer').then((m) => ({ default: m.ResponseViewer })),
);

// Settings is a whole panel behind a tab; never needed to boot.
const SettingsPanel = lazy(() =>
  import('@/features/settings/SettingsPanel').then((m) => ({ default: m.SettingsPanel })),
);
const EntitySettings = lazy(() =>
  import('@/features/workspace/EntitySettings').then((m) => ({ default: m.EntitySettings })),
);

// Modals: mounted in the tree but only visible in response to a user
// action, so they are pure dead weight at boot.
//
// `NewItemModal` and `BootstrapErrorBanner` are NOT here — both are
// reachable from the welcome screen too, so `App.tsx` owns one instance
// of each above the surface switch. Duplicating them would mount two
// portals for the same store state.
const DirtyCloseDialog = lazy(() =>
  import('@/features/workspace/DirtyCloseDialog').then((m) => ({ default: m.DirtyCloseDialog })),
);

/**
 * Placeholder for a chunk that has not arrived yet.
 *
 * Sized to fill its slot rather than a centred spinner: a lazy panel that
 * collapses to a spinner and then snaps to full height is a layout jump,
 * which is exactly the jank this whole change exists to remove.
 */
function PanelFallback({ label }: { label?: string }) {
  return (
    <div
      className="flex min-h-0 flex-1 items-center justify-center text-xs text-muted-foreground"
      aria-busy="true"
    >
      {label ?? <span className="opacity-60">加载中…</span>}
    </div>
  );
}

export function WorkspaceShell() {
  const activeTab = useAppStore((s) => s.openTabs.find((t) => t.id === s.activeTabId) ?? null);
  const activeProjectId = useAppStore((s) => s.activeProjectId);

  // Local splitter state — HttpTester (top) / ResponseViewer (bottom) ratio.
  // Only relevant while the active tab is an endpoint; for project/module/
  // collection tabs the body is a single scroll area.
  const [basis, setBasis] = useState<string>('50%');
  const containerRef = useRef<HTMLDivElement>(null);
  const resetBasis = () => setBasis('50%');

  // Warm the editor chunks during the bootstrap window.
  //
  // This shell is itself lazy, so by the time it mounts the module graph
  // is already resolving; issuing the same dynamic imports the lazy
  // components use populates the registry, so by the time the store has
  // an endpoint tab to render, the CodeMirror chunk is already decoded
  // and the `lazy()` boundary resolves synchronously. Same specifier →
  // same chunk, so this costs one fetch that would have happened anyway,
  // just earlier and off the critical path.
  //
  // Gated on an endpoint being the active tab. Warming unconditionally
  // would pull ~450 kB of editor into a workspace that only ever shows a
  // project card.
  useEffect(() => {
    if (activeTab?.kind !== 'endpoint') return;
    const t = setTimeout(() => {
      void import('@/features/http-tester/BodyEditor');
      void import('@/features/response-viewer/ResponseViewer');
      void import('@/features/http-tester/DocsEditor');
    }, 0);
    return () => clearTimeout(t);
  }, [activeTab?.kind]);

  // Choose what to render in the right pane based on the active tab kind.
  //   endpoint → request/response split
  //   project / module / collection → EntitySettings: the entity's own
  //     single-column pane (identity, base info, and whatever configuration
  //     belongs to that kind — the project's environments, the module's
  //     per-environment baseUrls)
  //   settings → the settings panel (now a tab itself, not a drawer)
  //
  // No active tab with a project active is what closing the last tab
  // leaves behind: the project itself is the natural landing surface,
  // which is also where project rename / delete now live, since the
  // project is no longer a row in the tree. With *no* project and no tab
  // there is nothing to show — but `App.tsx` renders the welcome screen
  // in that state instead, so it never reaches here.
  const renderActiveContent = () => {
    if (!activeTab) {
      return (
        <Suspense fallback={<PanelFallback />}>
          <EntitySettings kind="project" entityId={activeProjectId} />
        </Suspense>
      );
    }
    // A draft endpoint renders the same tester as a saved one. The two are
    // deliberately not separate components: `useActiveEndpointResource`
    // hands both over in one shape, so the tester runs a single code path
    // and an unsaved endpoint is editable, sendable and previewable with
    // the same UI as a saved one.
    if (activeTab.kind === 'endpoint' || activeTab.kind === 'endpoint-draft') {
      return (
        <>
          <div
            style={{ flexBasis: basis, flexGrow: 0, flexShrink: 0 }}
            className="flex min-h-0 flex-col"
          >
            <HttpTester />
          </div>
          <Splitter
            direction="horizontal"
            containerRef={containerRef}
            basis={basis}
            onResize={setBasis}
            onReset={resetBasis}
            minSize={240}
            minOtherSize={160}
          />
          <div className="min-h-0 flex-1">
            <Suspense fallback={<PanelFallback />}>
              <ResponseViewer />
            </Suspense>
          </div>
        </>
      );
    }
    if (activeTab.kind === 'settings') {
      return (
        <Suspense fallback={<PanelFallback />}>
          <SettingsPanel />
        </Suspense>
      );
    }
    return (
      <Suspense fallback={<PanelFallback />}>
        <EntitySettings kind={activeTab.kind} entityId={activeTab.id} />
      </Suspense>
    );
  };

  return (
    <div className="flex h-full min-h-0 w-full flex-col">
      <TopBar />
      <main className="flex min-h-0 flex-1">
        {/* No project → no tree. See the module header for why the
            placeholder column was worse than nothing. */}
        {activeProjectId !== '' && <ProjectTree />}
        <div ref={containerRef} className="flex min-w-0 flex-1 flex-col">
          <WorkspaceTabs />
          {renderActiveContent()}
        </div>
      </main>
      <Suspense fallback={null}>
        <DirtyCloseDialog />
      </Suspense>
    </div>
  );
}
