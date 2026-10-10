import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import { ingestBearerFromUrl } from './lib/tauri-commands';
import { useAppStore } from './stores/app-store';
import './styles/globals.css';

// When the reverse-proxy (or a hand-shared link) points at
// `?token=<x>`, persist the bearer before the React tree mounts so
// the very first `webFetch()` already carries the header. `ingest`
// also strips the query from `window.history` so the token doesn't
// end up in browser history / Referer headers / bookmarks.
ingestBearerFromUrl();

// ---------------------------------------------------------------------------
// Apply the persisted theme to <html> before React mounts. The store loads
// preferences synchronously from localStorage at module init, so we can
// read the resolved theme here and toggle the `.dark` class without
// causing a dark → light flash for users with light/system saved.
// ---------------------------------------------------------------------------
const PREFS_KEY = 'simple-post:prefs';
type Theme = 'dark' | 'light' | 'system';
function resolveInitialTheme(): Theme {
  try {
    const raw = window.localStorage.getItem(PREFS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as { theme?: Theme };
      if (parsed?.theme === 'dark' || parsed?.theme === 'light' || parsed?.theme === 'system') {
        return parsed.theme;
      }
    }
  } catch {
    // ignore — fall back to default
  }
  return 'dark';
}
{
  const theme = resolveInitialTheme();
  const root = document.documentElement;
  const wantDark =
    theme === 'dark' ||
    (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
  root.classList.toggle('dark', wantDark);
}

// ---------------------------------------------------------------------------
// Start the backend round-trip BEFORE React mounts.
//
// `bootstrap()` used to be triggered from `TopBar`'s mount effect, which runs
// only after React has parsed and executed the whole initial module graph and
// committed the tree. That serialised the two biggest startup costs — fetching
// the workspace and rendering the shell — when they have no dependency on each
// other.
//
// Firing it here overlaps the IPC with `createRoot` and with the editor chunks
// that `App` warms up on mount. `bootstrap()` is idempotent and returns
// immediately when `loaded || loading`, so a later call is a no-op rather than
// a duplicate fetch; the store is a module singleton, so the state this
// writes is the same one the tree reads.
//
// Both transports are ready at module-eval time: web mode uses `fetch`, and
// Tauri mode uses `window.__TAURI_INTERNALS__`, which Tauri injects before any
// user script runs.
// ---------------------------------------------------------------------------
void useAppStore.getState().bootstrap();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
