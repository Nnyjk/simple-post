// Tailwind v4 的 PostCSS 插件换成了独立包 `@tailwindcss/postcss`——
// `tailwindcss` 这个入口在 v4 里不再导出 PostCSS 插件，直接用它会报
// 「The PostCSS plugin has moved to a separate package」。
//
// v4 自带 Lightning CSS 做浏览器前缀处理，autoprefixer 不再需要。
export default {
  plugins: {
    '@tailwindcss/postcss': {},
  },
};
