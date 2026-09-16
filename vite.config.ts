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
