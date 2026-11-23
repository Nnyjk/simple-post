/**
 * Tailwind v4 的主题配置。
 *
 * v4 把主题搬进了 CSS 的 `@theme`，本项目用 `src/styles/globals.css` 里的
 * `@config "../../tailwind.config.js"` 继续读这个文件——原因见该文件顶部的
 * 注释（v4 改了圆角刻度的名字，配置留在 JS 里就不必逐个改名）。
 *
 * **tailwind-merge 与本文件是配对的，改一个必须改另一个。**
 * tailwind-merge 2.x 只认 Tailwind v3 的类名表，3.x 只认 Tailwind v4 的类名表。
 * 配错的后果是 `cn()` 用错规则表判定「谁覆盖谁」，而这种错误不报任何错。
 *
 * 具体一例：tailwind-merge 3.x 认为 v3 的兼容名 `bg-gradient-to-br` 与
 * `bg-<颜色>` 互相覆盖（`cn('bg-gradient-to-br', 'bg-primary')` 只剩
 * `bg-primary`，渐变被丢掉），而 v4 的原生名 `bg-linear-to-br` 不与颜色冲突。
 * 两个名字在 v4 的 CSS 里都还能用，故本项目一律用 `bg-linear-*`。
 *
 * @type {import('tailwindcss').Config} Tailwind 配置类型定义
 */
export default {
  darkMode: 'class',
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        // shadcn/ui 中性灰阶色
        border: 'hsl(var(--border))',
        input: 'hsl(var(--input))',
        ring: 'hsl(var(--ring))',
        background: 'hsl(var(--background))',
        foreground: 'hsl(var(--foreground))',
        muted: {
          DEFAULT: 'hsl(var(--muted))',
          foreground: 'hsl(var(--muted-foreground))',
        },
        accent: {
          DEFAULT: 'hsl(var(--accent))',
          foreground: 'hsl(var(--accent-foreground))',
        },
        popover: {
          DEFAULT: 'hsl(var(--popover))',
          foreground: 'hsl(var(--popover-foreground))',
        },
        card: {
          DEFAULT: 'hsl(var(--card))',
          foreground: 'hsl(var(--card-foreground))',
        },
        // 主品牌色（按钮、活动 Tab、徽标、聚焦环等）。定义为独立 HSL 变量以支持 JSX 类名与 @apply。
        primary: {
          DEFAULT: 'hsl(var(--primary))',
          foreground: 'hsl(var(--primary-foreground))',
        },
        // 语义状态色。由 globals.css 的 CSS 变量驱动明暗模式，避免在组件中硬编码十六进制色值。
        success: {
          DEFAULT: 'hsl(var(--success))',
          foreground: 'hsl(var(--success-foreground))',
        },
        warning: {
          DEFAULT: 'hsl(var(--warning))',
          foreground: 'hsl(var(--warning-foreground))',
        },
        destructive: {
          DEFAULT: 'hsl(var(--destructive))',
          foreground: 'hsl(var(--destructive-foreground))',
        },
        // 三级弱文本（时间戳、计数、面包屑等展示类信息），保留 muted-foreground 用于说明性文本。
        faint: 'hsl(var(--text-faint))',
        // 悬浮与选中态（半透明表面用 alpha 叠加，不透明表面用实体填充），避免手写 bg-accent/40。
        'overlay-hover': 'var(--overlay-hover)',
        'overlay-select': 'var(--overlay-select)',
        'surface-hover': 'hsl(var(--surface-hover))',
        'surface-select': 'hsl(var(--surface-select))',
        // 窗口边框 1px 描边：Windows 自带阴影在暗色表面不易分辨，需显式绘制边框。
        'edge-window': 'hsl(var(--edge-window))',
        // 分段控件滑块高亮背景。暗色模式下滑块与底槽的明度关系与 card 相反，必须保持独立键名。
        chip: 'hsl(var(--chip))',
        // HTTP 方法色不在配置中静态定义：由 MethodPill 在运行时通过内联 style 绑定 CSS 变量（真值见 src/lib/utils.ts 的 methodColorVar）。
        //
        // 资源语义色相（project / module / collection / endpoint / baseurl / variable / connection / request）收拢在 hue 下（概念映射见 src/lib/hue.ts）。
        hue: {
          project: 'hsl(var(--hue-project))',
          module: 'hsl(var(--hue-module))',
          collection: 'hsl(var(--hue-collection))',
          endpoint: 'hsl(var(--hue-endpoint))',
          baseurl: 'hsl(var(--hue-baseurl))',
          variable: 'hsl(var(--hue-variable))',
          connection: 'hsl(var(--hue-connection))',
          request: 'hsl(var(--hue-request))',
        },
      },
      // 显式圆角阶梯（卡片与容器 6px，控件 4px，内部徽标 3px），不使用相对计算链，贴合 Windows 桌面规范。
      borderRadius: {
        pill: 'var(--radius-pill)',
        lg: 'var(--radius)', // 6px：卡片、列表行、弹出层、菜单
        md: '4px', // 4px：按钮、输入框、分段项
        sm: '3px', // 3px：内部徽标与细节
        DEFAULT: '4px', // 默认控件圆角
      },
      // 字号阶梯整体上调一档：text-xs 设为 13px（适配资源树、Tab 栏、设置导航），保持桌面端排版密度；text-sm 维持 14px。
      fontSize: {
        xs: ['0.8125rem', { lineHeight: '1.25rem' }], // 13px
        sm: ['0.875rem', { lineHeight: '1.25rem' }], // 14px
        base: ['1rem', { lineHeight: '1.5rem' }], // 16px
        lg: ['1.125rem', { lineHeight: '1.625rem' }], // 18px：分组标题
        '2xl': ['1.75rem', { lineHeight: '2rem' }], // 28px：欢迎屏标题
        // 11px / 10px。全仓 219 处手写 `text-[11px]` / `text-[10px]` 已迁到这两个名字
        // 上，「任意字号」由 `src/lib/__tests__/style-guards.test.ts` 禁止 —— 再要新档位
        // 先加在这里，别在组件里发明 `text-[9px]`（中文字形在那之下糊成一片）。
        // `lib/utils.ts` 的 `cn()` 已把这两个名字登记进 tailwind-merge 的 font-size 组，
        // 否则 `cn('text-xs', 'text-nano')` 会留下两个类、样式表顺序说了算。
        // 例外只在 `src/styles/globals.css` 的 `@apply` 里（prose 与表单内部样式）。
        micro: ['0.6875rem', { lineHeight: '1rem' }], // 11px
        nano: ['0.625rem', { lineHeight: '0.875rem' }], // 10px
      },
      fontFamily: {
        sans: [
          'Inter',
          'system-ui',
          '-apple-system',
          'Segoe UI',
          'PingFang SC',
          'Hiragino Sans GB',
          'Microsoft YaHei',
          'sans-serif',
        ],
        mono: ['JetBrains Mono', 'Fira Code', 'Cascadia Code', 'Consolas', 'monospace'],
      },
      keyframes: {
        'fade-in': {
          from: { opacity: '0', transform: 'translateY(2px)' },
          to: { opacity: '1', transform: 'translateY(0)' },
        },
        'slide-in-from-top': {
          from: { transform: 'translateY(-8px)', opacity: '0' },
          to: { transform: 'translateY(0)', opacity: '1' },
        },
      },
      animation: {
        'fade-in': 'fade-in 150ms ease-out',
        'slide-in-from-top': 'slide-in-from-top 200ms ease-out',
      },
    },
  },
  plugins: [],
};
