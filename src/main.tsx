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
// 在 React 挂载之前发起后端往返。
//
// `bootstrap()` 放在这里而不是某个组件的挂载副作用里：组件挂载要等 React 解析并执行
// 完整个初始模块图再提交树，那会把「取工作区」与「渲染外壳」两段互不依赖的开销串成
// 先后。这里提前发起，IPC 与 `createRoot`、与 `App` 挂载时预热的编辑器块重叠执行。
//
// `bootstrap()` 是幂等的，`loaded || loading` 时立即返回，所以后面再调用一次是空操作
// 而不是重复取数；store 是模块单例，这里写入的状态就是组件树读到的同一份。
//
// 两种传输在模块求值时就绪：web 模式用 `fetch`，Tauri 模式用
// `window.__TAURI_INTERNALS__`（Tauri 在任何用户脚本之前注入它）。
// ---------------------------------------------------------------------------
void useAppStore.getState().bootstrap();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
