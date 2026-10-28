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
// 主题由 `index.html` 的 `<head>` 里那段内联脚本在 HTML 解析期就写到 `<html>` 上了。
//
// 那段脚本是构建期注入的，源码在 `src/lib/theme-boot/index.ts`（见 `vite.config.ts` 的
// `themeBootPlugin`）——偏好键与主题规则在整个仓库只有那一处。`App.tsx` 的挂载副作用会
// 再写一次，那一次管的是「用户在设置里改了主题」之后的那次写入。
// ---------------------------------------------------------------------------

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
