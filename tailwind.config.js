/** @type {import('tailwindcss').Config} */
export default {
  darkMode: 'class',
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        // shadcn/ui-style neutrals
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
        // Primary brand color (used by buttons, active tab bar, badges,
        // focus rings, etc.). Defined as its own HSL variable so the
        // `text-primary` / `bg-primary` / `border-primary` utilities work
        // both in JSX className and in @apply.
        primary: {
          DEFAULT: 'hsl(var(--primary))',
          foreground: 'hsl(var(--primary-foreground))',
        },
        // Semantic status colors — light/dark aware via the matching
        // CSS variables in src/styles/globals.css. Components reach for
        // `bg-success / text-warning-foreground / border-destructive/30`
        // rather than hardcoded emerald-500/amber-500/red-500 hexes so
        // dark-mode and future theme tweaks stay centralized.
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
        // HTTP method colors deliberately do NOT live here. `MethodPill`
        // paints its swatch through an inline `style` because the mapping
        // has to be computed per-method at runtime (see `methodColorVar` in
        // src/lib/utils.ts), which a static Tailwind color key cannot
        // express. An earlier `method: { … }` block existed here and was
        // removed: nothing ever emitted a `*-method-*` class, and it had
        // silently drifted from `methodColorVar` (POST/PUT were swapped).
        // Single source of truth: src/lib/utils.ts.
        //
        // Semantic hue accents (project / module / collection / endpoint /
        // baseUrl / variable / connection / request) DO live here, under
        // `hue` — see src/lib/hue.ts for the concept→class map.
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
      borderRadius: {
        lg: 'var(--radius)',
        md: 'calc(var(--radius) - 2px)',
        sm: 'calc(var(--radius) - 4px)',
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
