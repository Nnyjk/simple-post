import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'node:path';

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    // 暴露在所有网卡接口，方便局域网内其他设备访问
    host: '0.0.0.0',
    // 允许任意 Host 头（Vite 5 默认只允许 localhost/127.0.0.1，局域网 IP 会被拦）
    allowedHosts: true,
    // 局域网访问时日志只显示来源 IP 而不是本地
    cors: true,
    // 把 /api 和 /mcp 透传到本地 Rust serve-web（axum，127.0.0.1:4173），
    // 这样 Agent / 浏览器打开 localhost:5173 时，前端 fetch('/api/...') 会
    // 由 Vite 转发到 Rust，而不是命中 Vite 自己的 dev server（404）。
    //
    // 注意：必须先 `cargo run -- serve-web`（或用 `npm run dev:split` 一
    // 次性起两个进程）。如果 serve-web 没起来，代理会 502。
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:4173',
        changeOrigin: true,
        // 不要 rewrite：/api/* 直接透传给 Rust 的 axum 路由（也是 /api/*）。
      },
      '/mcp': {
        target: 'http://127.0.0.1:4173',
        changeOrigin: true,
        // MCP 用的是 HTTP JSON-RPC，不是 WebSocket；显式关闭 ws 防止 Vite 5
        // 把它当成 WS upgrade 处理导致握手失败。
        ws: false,
      },
    },
  },
  clearScreen: false,
  envPrefix: ['VITE_', 'TAURI_'],
  optimizeDeps: {
    /**
     * Pre-bundle only what the first paint needs, and stop the crawler.
     *
     * With the default discovery, Vite walks the import graph from the
     * entry and pre-bundles everything it finds — and it follows
     * `import()`, so `lazy()` alone does not keep a package out. That
     * pulls the whole CodeMirror stack, `gsap` + `Flip`, `marked` and a
     * 1.1 MB `lucide-react` into the startup cost of a screen that uses
     * none of them. Measured on this machine: 11.3 s for the first
     * request cold, against 139 ms with the config below. The production
     * build is unaffected either way — it boots in ~66 ms.
     *
     * `noDiscovery` stops the crawl; `include` then names exactly the
     * deps the first screen renders. Everything else is served as raw
     * ESM when it is actually imported, which now only happens after a
     * project is opened.
     *
     * The trade: the first time a user opens the HTTP tester they pay a
     * few hundred ms of unbundled module requests. Worth it when the
     * startup screen is on every launch and the editor is not.
     *
     * Do NOT add the editor packages back here "to be safe". Pre-bundling
     * is eager once a package is named, so listing them reintroduces the
     * exact cost above.
     */
    noDiscovery: true,
    include: [
      // The startup path, in the order `main.tsx` reaches it.
      'react',
      'react-dom',
      'react-dom/client',
      'zustand',
      'lucide-react',
      'clsx',
      'tailwind-merge',
    ],
  },
  build: {
    rollupOptions: {
      output: {
        // Only pin chunks that are genuinely shared by *several* statically
        // loaded entry points, where a stable vendor chunk helps HTTP cache
        // reuse across app releases.
        //
        // CodeMirror and `marked` used to be pinned here too. That defeated
        // the code splitting: `manualChunks` hoists the listed modules (and
        // their dependency closure) into one chunk, and Rollup then emits a
        // *static* import for that chunk from any statically-loaded importer
        // — which put 425 kB of editor back on the critical path even after
        // `BodyEditor` / `ResponseViewer` / `DocsEditor` became lazy. Left to
        // Rollup's automatic splitting they now sit in async chunks, which is
        // the entire point. To verify, read the emitted chunk graph in
        // `dist/assets` and confirm no statically-imported entry names
        // these packages — a chunk *listing* alone will not show a static
        // import edge.
        manualChunks: {
          react: ['react', 'react-dom'],
          state: ['zustand'],
        },
      },
    },
  },
});
