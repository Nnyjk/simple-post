import { Suspense, lazy, useEffect } from 'react';
import { WelcomeScreen } from '@/features/welcome/WelcomeScreen';
import { useAppStore } from '@/stores/app-store';
import { BootSplashGate } from '@/features/welcome/BootSplashGate';

// ---------------------------------------------------------------------------
// Two surfaces, one store.
//
// The app has exactly two screens, and this file picks between them on
// `activeProjectId` and nothing else:
//
//   activeProjectId === ''  → WelcomeScreen   (full window, no chrome)
//   otherwise               → WorkspaceShell  (top bar + tree + tabs)
//
// A conditional inside a shared layout is the thing to avoid: it leaves
// the welcome screen a page squeezed into a pane, surrounded by chrome
// that cannot do anything without a project.
//
// The split buys three things:
//
//   1. Speed. The workspace — top bar, project tree, HTTP tester,
//      settings panels, and through them the CodeMirror editors — is
//      `lazy()`. Opening the app fetches the store and the picker; the
//      rest arrives only if a project is actually entered.
//
//   2. Data. `bootstrap()` has two layers (see `bootstrap-slice.ts`).
//      Layer 1 is the project list and it alone tears down the boot
//      splash; layer 2 is the tree and its configuration and it streams
//      in behind the welcome screen.
//
//   3. No dead UI. With no project there is no tree, no env switcher and
//      no empty right pane — the welcome page owns the whole window.
//
// Everything global stays here rather than inside either shell: the theme
// effect (the welcome page is themed too), the Ctrl+Enter / Ctrl+S hotkeys
// (the workspace owns the buttons they click, but the listener must survive
// a shell swap), and the two overlays reachable from both surfaces — the
// create-item modal and the startup-error banner.
// ---------------------------------------------------------------------------

// The whole workspace frame. Its chunk pulls in the tree, the top bar,
// the HTTP tester, the settings panels and — through those — the
// CodeMirror editors. None of it is reachable until a project is open.
const WorkspaceShell = lazy(() =>
  import('@/features/workspace/WorkspaceShell').then((m) => ({ default: m.WorkspaceShell })),
);

// A startup failure can land on either surface, so the banner sits
// above both rather than inside the shell that happens to be mounted.
const BootstrapErrorBanner = lazy(() =>
  import('@/features/top-bar/BootstrapErrorBanner').then((m) => ({
    default: m.BootstrapErrorBanner,
  })),
);

// Portal-rendered and driven entirely by `newItem.open`, so one instance
// serves both surfaces: the welcome page's "新建项目" and the tree's
// "新建模块" are the same dialog.
const NewItemModal = lazy(() =>
  import('@/features/project-tree/NewItemModal').then((m) => ({ default: m.NewItemModal })),
);

/**
 * Placeholder for the workspace chunk.
 *
 * Sized to fill its slot rather than a centred spinner: a shell that
 * collapses to nothing and then snaps to full height is a layout jump.
 * It also carries the boot splash's visual language so the hand-off
 * reads as continuous.
 */
function WorkspaceFallback() {
  return (
    <div className="flex h-full w-full items-center justify-center text-xs text-muted-foreground">
      <span className="opacity-60">加载工作区…</span>
    </div>
  );
}

function App() {
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const theme = useAppStore((s) => s.theme);
  // Gate the two overlays on the state that makes them visible rather
  // than mounting them unconditionally.
  //
  // A `lazy()` component fires its import the moment it is *rendered*, so
  // `<Suspense><NewItemModal /></Suspense>` at startup still downloads
  // the chunk during the first paint — it just doesn't block it. The
  // create-item modal pulls in the dialog primitive, the select, and
  // through those GSAP (~38 kB gzip), none of which the welcome screen
  // needs. Mounting on `open` makes the fetch happen on the click that
  // needs it instead.
  const bootstrapError = useAppStore((s) => s.bootstrapError);
  const newItemOpen = useAppStore((s) => s.newItem.open);

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

  // Global hotkeys — Ctrl+Enter (send), Ctrl+S (save).
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

  const onWelcome = activeProjectId === '';

  return (
    <div className="flex h-screen w-screen flex-col bg-background text-foreground">
      {/* Removes the static HTML splash once the store can render real data
          (or has failed). See features/welcome/BootSplashGate.tsx. */}
      <BootSplashGate />
      {/* Spans both surfaces: layer 1 of the bootstrap can fail with the
          welcome screen mounted, in which case there is no top bar to
          host it. Mounted only once there is an error to show. */}
      {bootstrapError !== null && (
        <Suspense fallback={null}>
          <BootstrapErrorBanner />
        </Suspense>
      )}
      {onWelcome ? (
        <WelcomeScreen />
      ) : (
        <Suspense fallback={<WorkspaceFallback />}>
          <WorkspaceShell />
        </Suspense>
      )}
      {/* Portal-rendered and driven entirely by `newItem.open`, so one
          instance serves both surfaces: the welcome page's "新建项目" and
          the tree's "新建模块" are the same dialog. */}
      {newItemOpen && (
        <Suspense fallback={null}>
          <NewItemModal />
        </Suspense>
      )}
    </div>
  );
}

export default App;
