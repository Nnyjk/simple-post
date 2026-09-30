/**
 * Semantic hue accents — the single concept→colour map.
 *
 * WHY THIS FILE EXISTS
 * --------------------
 * "Which colour is a module?" had four different answers depending on
 * where you looked:
 *
 *   project-tree/rows/ModuleRow.tsx   an amber palette pair
 *   workspace/WorkspaceTabs.tsx       iconColor: '#fbbf24'   (inline hex)
 *   workspace/EntitySettings.tsx      text-warning           (a *status* token)
 *   settings/sections/rows/…          the same amber pair as the tree
 *
 * Two of those didn't even follow the theme: the inline hex is a fixed
 * value, so a module's tab icon stayed amber-400 in dark mode where the
 * tree row had already shifted. This module routes every one of those
 * decisions through `HUE`, and `globals.css` owns the actual values as
 * CSS variables so light/dark swap automatically with the `.dark` class.
 *
 * TWO TIERS
 * ---------
 * Tier A — project / module / collection / endpoint. More chroma. These
 * are the tree's structural vocabulary; the icons already differ in
 * shape (package / folder / open-folder / code), so colour reinforces
 * rather than carries the meaning.
 *
 * Tier B — baseurl / variable / connection / request. Deliberately
 * quieter, so settings and config chrome never competes with the tree
 * for attention. Low chroma also keeps `connection`'s rose from being
 * mistaken for the saturated `--destructive` red that sits right beside
 * it in the same panel.
 *
 * Two further concepts deliberately take NO hue:
 *   - environment  → `--primary` (it is the app's primary context, and
 *     reusing primary is semantically honest rather than a collision)
 *   - appearance   → `--muted-foreground` (a cosmetic preference panel;
 *     giving it a hue made it shout for no informational gain)
 *
 * HUE ANGLES ARE LOAD-BEARING
 * ---------------------------
 * These accents render at 12-14px behind a 15%-alpha tint, where
 * lightness differences are nearly invisible and hue distance is the only
 * cue a viewer can still resolve. The angles below are therefore spaced
 * so that no two *co-occurring* concepts land within 30° of each other —
 * see HUE_ANGLES and the separation assertions in
 * `src/lib/__tests__/hue.test.ts`. Adjusting saturation or lightness is
 * safe; rotating a hue is not, because it re-collides two concepts.
 */

export type HueConcept =
  | 'project'
  | 'module'
  | 'collection'
  | 'endpoint'
  | 'baseurl'
  | 'variable'
  | 'connection'
  | 'request';

/**
 * Hue angle in degrees, per concept. The single source of truth for the
 * *separation* property; the concrete colour triples live in
 * `src/styles/globals.css` under `--hue-*`.
 *
 * The four pairs that used to collide (and their new separation):
 *   baseurl ↔ collection   40°  (were both `sky`)
 *   variable ↔ module      50°  (were both `amber`)
 *   connection ↔ project   90°  (were both `violet`)
 *   request ↔ endpoint    160°  (were both `emerald`)
 */
export const HUE_ANGLES: Record<HueConcept, number> = {
  project: 265,
  module: 45,
  collection: 185,
  endpoint: 150,
  baseurl: 225,
  variable: 95,
  connection: 355,
  request: 310,
};

/** Concept pairs that appear in the same list/panel and must stay apart. */
export const HUE_SEPARATION_PAIRS: readonly (readonly [HueConcept, HueConcept])[] = [
  ['baseurl', 'collection'],
  ['variable', 'module'],
  ['connection', 'project'],
  ['request', 'endpoint'],
];

/** Minimum hue angle, in degrees, that two co-occurring concepts must keep. */
export const HUE_MIN_SEPARATION = 30;

export interface HueClasses {
  /** 15%-alpha tint used as the tile's background. */
  bg: string;
  /** Full-chroma foreground — icon glyph and text. */
  text: string;
  /** 30%-alpha edge, for tiles that carry a border. */
  border: string;
}

/**
 * Concept → utility classes. The three entries per concept are kept
 * adjacent on purpose: changing one without the others is exactly the
 * drift this file exists to prevent, and a test asserts they always
 * reference the same `--hue-*` variable.
 */
export const HUE: Record<HueConcept, HueClasses> = {
  project: {
    bg: 'bg-hue-project/15',
    text: 'text-hue-project',
    border: 'border-hue-project/30',
  },
  module: {
    bg: 'bg-hue-module/15',
    text: 'text-hue-module',
    border: 'border-hue-module/30',
  },
  collection: {
    bg: 'bg-hue-collection/15',
    text: 'text-hue-collection',
    border: 'border-hue-collection/30',
  },
  endpoint: {
    bg: 'bg-hue-endpoint/15',
    text: 'text-hue-endpoint',
    border: 'border-hue-endpoint/30',
  },
  baseurl: {
    bg: 'bg-hue-baseurl/15',
    text: 'text-hue-baseurl',
    border: 'border-hue-baseurl/30',
  },
  variable: {
    bg: 'bg-hue-variable/15',
    text: 'text-hue-variable',
    border: 'border-hue-variable/30',
  },
  connection: {
    bg: 'bg-hue-connection/15',
    text: 'text-hue-connection',
    border: 'border-hue-connection/30',
  },
  request: {
    bg: 'bg-hue-request/15',
    text: 'text-hue-request',
    border: 'border-hue-request/30',
  },
};
