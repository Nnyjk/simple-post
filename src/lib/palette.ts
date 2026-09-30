/**
 * Project accent colours — the palette offered when creating a project.
 *
 * This used to be duplicated in three places: a 4-entry list in
 * `NewItemModal`, a 6-entry list inside the (now removed)
 * `components/ui/section.tsx`, and a bare `'#60a5fa'` literal repeated
 * three times in `ui-slice` as the new-project default. The two lists had
 * already drifted apart in length, and the default literal was a fourth
 * hand-typed value. One list now owns all of it.
 *
 * The values are picked to be legible on both light and dark card
 * backgrounds and to stay clear of the semantic `hue` accents used by the
 * resource tree (see `src/lib/hue.ts`) — a project's own colour should
 * read as decoration, not as a node kind.
 *
 * `PROJECT_COLOR_DEFAULT` is also the `color` seeded into a brand-new
 * project, so importing it from the store is safe: `ui-slice` already
 * depends on this module and nothing here imports the store.
 */
export const PROJECT_COLORS = [
  '#60a5fa', // blue
  '#a78bfa', // violet
  '#34d399', // emerald
  '#fbbf24', // amber
  '#f87171', // red
  '#a3a3a3', // neutral
] as const;

/** Seed colour for a newly created project — the first swatch. */
export const PROJECT_COLOR_DEFAULT: string = PROJECT_COLORS[0];
