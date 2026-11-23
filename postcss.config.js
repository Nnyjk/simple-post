// Tailwind v4 的 PostCSS 插件换成了独立包 `@tailwindcss/postcss`——
// `tailwindcss` 入口在 v4 不再导出 PostCSS 插件，直接引入会提示插件已移至独立包。
//
// v4 自带 Lightning CSS 做浏览器前缀处理，autoprefixer 不再需要。
export default {
  plugins: {
    '@tailwindcss/postcss': {},
  },
};
