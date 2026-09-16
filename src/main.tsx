import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './styles/globals.css';

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

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
