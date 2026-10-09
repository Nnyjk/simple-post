/**
 * The Simple Post mark — the title-bar tile, at any size.
 *
 * This is the *same* geometry as `src-tauri/app-icon.svg`, the source
 * every platform icon is generated from (`npm run icons`). They are kept
 * in step deliberately: the app icon used to be a separate drawing, which
 * is exactly how a desktop icon ends up not matching the product. The
 * two files cross-reference each other — change one, change the other.
 *
 * Rendered inline rather than as an `<img>` so it inherits the theme's
 * `--primary` / `--hue-project` tokens and tracks light/dark with the
 * rest of the chrome. The generated PNGs cannot: they are baked at
 * generation time, which is why they use the light-theme values.
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
        {/* Tailwind's `bg-gradient-to-br` is `to bottom right`. */}
        <linearGradient id="sp-app-icon-gradient" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="hsl(var(--primary))" />
          <stop offset="100%" stopColor="hsl(var(--hue-project))" />
        </linearGradient>
      </defs>

      <rect width="1024" height="1024" rx="256" fill="url(#sp-app-icon-gradient)" />

      {/*
        Glyph at 14/24 = 0.583 of the tile — a `h-3.5 w-3.5` Sparkles
        inside its `h-6 w-6` box — so it scales to 597.33 units with
        213.33 of padding per side.
      */}
      <g
        transform="translate(213.33 213.33) scale(24.889)"
        fill="none"
        stroke="#FFFFFF"
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {/* lucide `sparkles`, verbatim from lucide-react 0.460 (ISC). */}
        <path d="M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z" />
        <path d="M20 3v4" />
        <path d="M22 5h-4" />
        <path d="M4 17v2" />
        <path d="M5 18H3" />
      </g>
    </svg>
  );
}
