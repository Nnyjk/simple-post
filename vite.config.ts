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
    // 允许任意 Host 头：局域网设备用本机 IP 打开时，Vite 会校验 Host 并拦下未列出的主机
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
        // MCP 用的是 HTTP JSON-RPC，不是 WebSocket；显式关闭 ws 防止 Vite
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
        // 只固定「被多个静态入口共用」的块，稳定的 vendor chunk 能让 HTTP 缓存在
        // 应用升级之间继续复用。
        //
        // CodeMirror 与 `marked` 不得出现在任何静态入口可达的块里：`manualChunks`
        // 会把列出的模块连同依赖闭包提升成一个块，而 Rollup 会为那个块发出
        // *静态* import，于是编辑器代码又回到首屏关键路径上。交给 Rollup 自动分包
        // 时它们落在异步块里。验证方式是读 `dist/assets` 里产出的块图，确认没有
        // 静态入口引用这两个包 —— 只看块的列表看不出静态 import 边。
        //
        // 必须保持为*函数*形式：Rolldown（Vite 8 的打包引擎）直接拒绝对象形式，
        // 报 `Expected Function but received Object` 与 `manualChunks is not a
        // function`。Vite 8 的配置兼容层会自动转换大部分 `rollupOptions`，唯独
        // 不转这一项。
        manualChunks(id: string) {
          if (/node_modules[\\/](react|react-dom|scheduler)[\\/]/.test(id)) return 'react';
          if (/node_modules[\\/]zustand[\\/]/.test(id)) return 'state';
          return undefined;
        },
      },
    },
  },
});
