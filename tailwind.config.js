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
        // Third text level, for information that is present rather than
        // actionable — timestamps, counts, breadcrumb trails. Gives
        // `muted-foreground` back to descriptive text. See globals.css.
        faint: 'hsl(var(--text-faint))',
        // Hover / selection, in the two flavours globals.css defines: an
        // alpha overlay for translucent surfaces, a solid fill for opaque
        // ones. Components should prefer these over a hand-written
        // `bg-accent/40` so the backdrop can change without a redesign.
        'overlay-hover': 'var(--overlay-hover)',
        'overlay-select': 'var(--overlay-select)',
        'surface-hover': 'hsl(var(--surface-hover))',
        'surface-select': 'hsl(var(--surface-select))',
        // 1px edge for the window frame. Windows' own drop shadow does not
        // read against a dark surface, so the edge is drawn explicitly.
        'edge-window': 'hsl(var(--edge-window))',
        // Raised thumb of a segmented control, inside a `bg-muted` trough.
        // Has to stay a *separate* key from `card`: on the surfaces these
        // sit on, `--card` and the thumb need opposite lightness in dark
        // mode, so aliasing one to the other would make the selected
        // segment vanish.
        chip: 'hsl(var(--chip))',
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
      // An explicit ladder rather than a derived one. The old chain
      // (`lg: var(--radius)`, `md: calc(-2px)`, `sm: calc(-4px)`) meant
      // rounding a card and rounding a button were always tied together,
      // which is the opposite of how native Windows does it: 8px for the
      // window, 6px for cards and rows, 4px for controls.
      borderRadius: {
        window: 'var(--radius-window)',
        pill: 'var(--radius-pill)',
        lg: 'var(--radius)', // 6px — cards, rows, popovers, menus
        md: '4px', // 4px — buttons, inputs, chips
        sm: '3px', // 3px — inner detail, e.g. a badge inside a chip
        DEFAULT: '4px', // controls; `rounded` reads as "a control"
      },
      // Type scale, shifted up one step. Chrome in this app was running at
      // 10–12px, which is what reads as "web app" rather than "desktop
      // app". Overriding here rather than editing every `text-xs` call
      // site means one file moves the whole interface.
      //
      // `text-xs` 12 → 13px is the load-bearing one (tree rows, tab strip,
      // settings nav). 14px is unchanged: this is a dense multi-panel
      // workspace with a 256px tree pane, so the 14px floor that suits a
      // search-box UI would start truncating here.
      fontSize: {
        xs: ['0.8125rem', { lineHeight: '1.25rem' }], // 13px
        sm: ['0.875rem', { lineHeight: '1.25rem' }], // 14px (unchanged)
        base: ['1rem', { lineHeight: '1.5rem' }], // 16px (unchanged)
        lg: ['1.125rem', { lineHeight: '1.625rem' }], // 18px — section titles
        '2xl': ['1.75rem', { lineHeight: '2rem' }], // 28px — welcome hero
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
