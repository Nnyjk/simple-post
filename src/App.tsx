import { useEffect, useRef, useState } from 'react';
import { TopBar } from '@/features/top-bar/TopBar';
import { ProjectTree } from '@/features/project-tree/ProjectTree';
import { HttpTester } from '@/features/http-tester/HttpTester';
import { ResponseViewer } from '@/features/response-viewer/ResponseViewer';
import { SettingsPanel } from '@/features/settings/SettingsPanel';
import { WelcomeScreen } from '@/features/welcome/WelcomeScreen';
import { NewItemModal } from '@/features/project-tree/NewItemModal';
import { WorkspaceTabs } from '@/features/workspace/WorkspaceTabs';
import { EntitySettings } from '@/features/workspace/EntitySettings';
import { DirtyCloseDialog } from '@/features/workspace/DirtyCloseDialog';
import { Splitter } from '@/components/ui/splitter';
import { useAppStore } from '@/stores/app-store';

function App() {
  const activeTab = useAppStore((s) =>
    s.openTabs.find((t) => t.id === s.activeTabId) ?? null,
  );
  const setRequestPending = useAppStore((s) => s.setRequestPending);
  const requestPending = useAppStore((s) => s.requestPending);
  const lastResponse = useAppStore((s) => s.lastResponse);
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

  // global hotkeys — Ctrl+Enter (send), Ctrl+S (save).
  // Ctrl+K (command palette) is handled inside <TopBar> so the palette can
  // share its open-state with the search box click.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const inField =
        target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.isContentEditable;

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
  const renderActiveContent = () => {
    if (!activeTab) return <WelcomeScreen />;
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
            <ResponseViewer />
          </div>
        </>
      );
    }
    if (activeTab.kind === 'settings') {
      return <SettingsPanel />;
    }
    return <EntitySettings kind={activeTab.kind} entityId={activeTab.id} />;
  };

  return (
    <div className="flex h-screen w-screen flex-col bg-background text-foreground">
      <TopBar />
      <main className="flex min-h-0 flex-1">
        <ProjectTree />
        <div ref={containerRef} className="flex min-w-0 flex-1 flex-col">
          <WorkspaceTabs />
          {renderActiveContent()}
        </div>
      </main>
      <DirtyCloseDialog />
      <NewItemModal />
    </div>
  );
}

export default App;
