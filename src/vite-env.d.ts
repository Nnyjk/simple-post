/// <reference types="vite/client" />

// TypeScript 6 起，侧效导入一个没有类型声明的模块（`import './x.css'`）报 TS2882。
// `vite/client` 声明了 `*.css` 与 `import.meta.env`，正好覆盖 main.tsx 的样式导入。
