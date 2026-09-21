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
  build: {
    rollupOptions: {
      output: {
        manualChunks: {
          // 把重型 vendor 拆出主 chunk,避免单文件 > 500kB 警告
          react: ['react', 'react-dom'],
          codemirror: [
            '@uiw/react-codemirror',
            '@codemirror/lang-json',
            '@codemirror/theme-one-dark',
          ],
          markdown: ['react-markdown'],
          state: ['zustand'],
        },
      },
    },
  },
});
