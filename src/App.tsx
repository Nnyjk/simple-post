import { Suspense, lazy, useEffect, useRef, useState } from 'react';
import { TopBar } from '@/features/top-bar/TopBar';
import { ProjectTree } from '@/features/project-tree/ProjectTree';
import { HttpTester } from '@/features/http-tester/HttpTester';
import { WelcomeScreen } from '@/features/welcome/WelcomeScreen';
import { WorkspaceTabs } from '@/features/workspace/WorkspaceTabs';
import { Splitter } from '@/components/ui/splitter';
import { useAppStore } from '@/stores/app-store';
import { BootSplashGate } from '@/features/welcome/BootSplashGate';

// ---------------------------------------------------------------------------
// Code splitting — see the note above each lazy boundary.
//
// Before this, every panel below was a static import, so `manualChunks`
// produced vendor chunks that were still *static* dependencies of the
// entry: the browser had to fetch and parse ~930 kB of JS (297 kB gzip,
// of which CodeMirror alone was 139 kB gzip) before React could mount
// anything at all. Splitting them turns that into a small shell plus
// parallel async chunks, so the window first becomes usable much sooner.
// ---------------------------------------------------------------------------

// CodeMirror (425 kB raw / 139 kB gzip). Only an endpoint tab needs it, and
// `bootstrap()` reopens the last endpoint on start, so this chunk is on the
// happy path — but it does not need to *block* the shell.
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

// Modals and banners: mounted unconditionally in the tree but only ever
// visible in response to a user action, so they are pure dead weight at boot.
const NewItemModal = lazy(() =>
  import('@/features/project-tree/NewItemModal').then((m) => ({ default: m.NewItemModal })),
);
const DirtyCloseDialog = lazy(() =>
  import('@/features/workspace/DirtyCloseDialog').then((m) => ({ default: m.DirtyCloseDialog })),
);
const BootstrapErrorBanner = lazy(() =>
  import('@/features/top-bar/BootstrapErrorBanner').then((m) => ({
    default: m.BootstrapErrorBanner,
  })),
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

function App() {
  const activeTab = useAppStore((s) => s.openTabs.find((t) => t.id === s.activeTabId) ?? null);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const theme = useAppStore((s) => s.theme);

  // Local splitter state — HttpTester (top) / ResponseViewer (bottom) ratio.
  // Only relevant while the active tab is an endpoint; for project/module/
  // collection tabs the body is a single scroll area.
  const [basis, setBasis] = useState<string>('50%');
  const containerRef = useRef<HTMLDivElement>(null);
  const resetBasis = () => setBasis('50%');

  // Apply the persisted theme to the <html> element. The store loads the
  // initial value synchronously from localStorage, so this runs once on
  // mount and again whenever the user changes theme from settings.
  useEffect(() => {
    const root = document.documentElement;
    const apply = (dark: boolean) => {
      root.classList.toggle('dark', dark);
    };
    if (theme === 'dark') {
      apply(true);
    } else if (theme === 'light') {
      apply(false);
    } else {
      // 'system' — follow OS preference, then keep in sync with changes.
      const mq = window.matchMedia('(prefers-color-scheme: dark)');
      apply(mq.matches);
      const onChange = (e: MediaQueryListEvent) => apply(e.matches);
      mq.addEventListener('change', onChange);
      return () => mq.removeEventListener('change', onChange);
    }
  }, [theme]);

  // Warm the editor chunks during the bootstrap window.
  //
  // `bootstrap()` fires before React mounts (see `main.tsx`) and its IPC
  // round-trip overlaps whatever the main thread is doing. Issuing the same
  // dynamic imports the lazy components use populates the module registry,
  // so by the time the store has data and an endpoint tab renders, the
  // CodeMirror chunk is already decoded and the `lazy()` boundary resolves
  // synchronously. Same specifier → same chunk, so this costs one fetch
  // that would have happened anyway, just earlier and off the critical path.
  //
  // Gated on an endpoint being the active tab. Warming unconditionally would
  // pull ~450 kB of editor into a brand-new workspace that only ever shows
  // the welcome screen — the exact waste this split exists to avoid. When an
  // endpoint *is* the active tab (which `bootstrap()` reopens for any
  // returning user), the editors are what the user is about to look at.
  useEffect(() => {
    if (activeTab?.kind !== 'endpoint') return;
    const t = setTimeout(() => {
      void import('@/features/http-tester/BodyEditor');
      void import('@/features/response-viewer/ResponseViewer');
      void import('@/features/http-tester/DocsEditor');
    }, 0);
    return () => clearTimeout(t);
  }, [activeTab?.kind]);

  // global hotkeys — Ctrl+Enter (send), Ctrl+S (save).
  // Ctrl+K (command palette) is handled inside <TopBar> so the palette can
  // share its open-state with the search box click.
  //
  // IME guard: `e.isComposing` is true while the IME composition
  // session is open (e.g. Chinese Pinyin, Japanese kana). Browsers
  // still dispatch `keydown` during composition; without skipping
  // these, a user mid-IME would accidentally fire send/save shortcuts
  // when their IME candidate window swallows the actual letter.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.isComposing) return;

      if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
        e.preventDefault();
        // trigger a synthetic send by re-using the button: we'll just dispatch click on the visible send button via document
        const btn = document.querySelector<HTMLButtonElement>('[data-testid="send-btn"]');
        btn?.click();
      }
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault();
        const btn = document.querySelector<HTMLButtonElement>('[data-testid="save-btn"]');
        btn?.click();
      }
      // Esc used to close the settings drawer. Settings is now a tab —
      // close it via the tab's X button, or by activating another tab.
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, []);

  // Choose what to render in the right pane based on the active tab kind.
  //   endpoint → request/response split
  //   project / module / collection → EntitySettings metadata editor
  //   settings → left-rail + main section (now a tab itself, not a drawer)
  //
  // No active tab is the app's *startup* state (bootstrap selects no
  // project) and also what closing the last tab leaves behind. Which of
  // the two it is depends on whether a project has been entered: without
  // one there is nothing to show but the picker, and with one the
  // project itself is the natural landing surface — which is also where
  // project rename / delete now live, since the project is no longer a
  // row in the tree.
  const renderActiveContent = () => {
    if (!activeTab) {
      return activeProjectId ? (
        <Suspense fallback={<PanelFallback />}>
          <EntitySettings kind="project" entityId={activeProjectId} />
        </Suspense>
      ) : (
        <WelcomeScreen />
      );
    }
    if (activeTab.kind === 'endpoint') {
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
    <div className="flex h-screen w-screen flex-col bg-background text-foreground">
      {/* Removes the static HTML splash once the store can render real data
          (or has failed). See features/welcome/BootSplashGate.tsx. */}
      <BootSplashGate />
      <TopBar />
      {/* Sits between the header and the workspace so a failed startup
          load is visible without covering the tree it made incomplete. */}
      <Suspense fallback={null}>
        <BootstrapErrorBanner />
      </Suspense>
      <main className="flex min-h-0 flex-1">
        <ProjectTree />
        <div ref={containerRef} className="flex min-w-0 flex-1 flex-col">
          <WorkspaceTabs />
          {renderActiveContent()}
        </div>
      </main>
      <Suspense fallback={null}>
        <DirtyCloseDialog />
        <NewItemModal />
      </Suspense>
    </div>
  );
}

export default App;
