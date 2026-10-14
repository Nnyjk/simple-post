/**
 * Simple Post 标记 —— 任意尺寸的标题栏 tile。
 *
 * 几何与 `src-tauri/app-icon.svg` **完全相同**，后者是各平台图标
 * （`npm run icons`）的生成源。两个文件互为交叉引用：改一个就得改另一个。
 *
 * 内联渲染而非 `<img>`，好让它继承主题的 `--primary` / `--hue-project` token，
 * 随其余 chrome 一起切换明暗。生成的 PNG 做不到——它们在生成时刻就烤定了，因此
 * 用的是亮色主题的值。
 */
import { cn } from '@/lib/utils';

export interface AppIconProps {
  className?: string;
}

export function AppIcon({ className }: AppIconProps) {
  return (
    <svg
      viewBox="0 0 1024 1024"
      className={cn('block', className)}
      role="img"
      aria-label="Simple Post"
    >
      <defs>
        {/* Tailwind 的 `bg-gradient-to-br` 就是 `to bottom right`。 */}
        <linearGradient id="sp-app-icon-gradient" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="hsl(var(--primary))" />
          <stop offset="100%" stopColor="hsl(var(--hue-project))" />
        </linearGradient>
      </defs>

      <rect width="1024" height="1024" rx="256" fill="url(#sp-app-icon-gradient)" />

      {/*
        字形取 tile 的 14/24 = 0.583——即 `h-3.5 w-3.5` 的 Sparkles 放进它的
        `h-6 w-6` 盒子里——于是缩放为 597.33 单位，每边留 213.33 内边距。
      */}
      <g
        transform="translate(213.33 213.33) scale(24.889)"
        fill="none"
        stroke="#FFFFFF"
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {/* lucide `sparkles`，逐字取自 lucide-react 0.460（ISC）。 */}
        <path d="M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z" />
        <path d="M20 3v4" />
        <path d="M22 5h-4" />
        <path d="M4 17v2" />
        <path d="M5 18H3" />
      </g>
    </svg>
  );
}
