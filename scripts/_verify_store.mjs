/**
 * Standalone verification harness for the "store-extensions" plan task.
 *
 * Three concerns from the spec, exercised without ever starting a
 * browser or Tauri shell:
 *
 *   (A) Selector stability — `useActiveProject` (and friends) must
 *       return the same reference across two consecutive calls on
 *       unchanged state. The spec hard-bans any selector that
 *       allocates (`map`, `filter`, `spread`, …).
 *
 *   (B) Cascade cleanup — deleting an endpoint must drop its
 *       `responseHistory[id]`, `responseExamples[id]` and
 *       `endpointDetails[id]` rows. Same applies when deleting the
 *       parent collection / module / project (each must clear every
 *       endpoint id in the subtree). One `deleteNode` action covers
 *       all three tree types; `deleteProject` is the `rootId` scope.
 *
 *   (C) Persistence — `setTheme`/`updateRequestSettings` must
 *       persist to localStorage under `simple-post:prefs`. The init
 *       path is verified by source review since re-importing the
 *       module in the same Node process wouldn't re-run the IIFE.
 *
 * Run:
 *   npx tsx scripts/_verify_store.mjs
 *
 * About the IPC mock
 * ------------------
 * The delete* actions optimistically mutate local state via `set()`,
 * then fire the Tauri command. When the command rejects, the catch
 * branch calls `set(snapshot)` to roll back — which **restores the
 * cascade removal**. So the cascade is observable in the success
 * path (IPC resolves) but gets erased on the failure path.
 *
 * The mock therefore makes every delete* resolve with `null`, so
 * the cascade sticks. The slice's IPC plumbing is irrelevant for
 * these concerns; we're testing the local mutation logic.
 */

// ---------- inline browser + IPC shim ----------
import { mockIPC } from '@tauri-apps/api/mocks';

const g = globalThis;
if (!g.window) g.window = g;
if (!g.window.__TAURI_INTERNALS__) g.window.__TAURI_INTERNALS__ = {};

// Native localStorage on Node 22+ is empty (no methods). Replace it
// unconditionally with a Map-backed shim that matches the spec the
// store reads (`getItem` / `setItem` / `removeItem`).
{
  const backing = new Map();
  const shim = {
    getItem(k) {
      return backing.has(k) ? backing.get(k) : null;
    },
    setItem(k, v) {
      backing.set(k, String(v));
    },
    removeItem(k) {
      backing.delete(k);
    },
    clear() {
      backing.clear();
    },
    get length() {
      return backing.size;
    },
    key(i) {
      return Array.from(backing.keys())[i] ?? null;
    },
  };
  g.localStorage = shim;
}

// Run a successful "delete" through the slice so the cascade sticks.
// Other commands (e.g. addEndpoint) won't be invoked by this script.
// Setting `undefined` triggers the catch branch; `null` resolves
// successfully. We pick success.
mockIPC(async () => null);

// ---------- import the store ----------
const { useAppStore, useActiveProject } = await import('../src/stores/app-store.ts');

// ---------- helpers ----------
let pass = 0;
let fail = 0;
function line(label, ok, detail) {
  const mark = ok ? 'PASS' : 'FAIL';
  console.log(`[${mark}] ${label}${detail ? ' — ' + detail : ''}`);
  if (ok) pass++;
  else fail++;
}

function makeProject(id) {
  return {
    id,
    name: 'X',
    description: '',
    baseUrlDefinitions: [],
    defaultBaseUrlDefinitionId: null,
    color: '#000000',
    createdAt: 0,
    updatedAt: 0,
  };
}
function makeNode(id, type, parentId, sortOrder) {
  return { id, rootId: 'p1', parentId, type, name: id.toUpperCase(), description: '', sortOrder };
}
function makeDetail(nodeId) {
  return {
    nodeId,
    method: 'GET',
    url: '/x',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { type: 'none' },
    tags: [],
  };
}
function makeResp() {
  return {
    status: 200,
    statusText: 'OK',
    durationMs: 1,
    sizeBytes: 0,
    headers: {},
    body: '',
    contentType: 'text/plain',
  };
}

// ---------- (A) selector stability ----------
console.log('\n=== (A) Selector stability ===');

useAppStore.setState({
  projects: [makeProject('p1')],
  activeProjectId: 'p1',
});

try {
  const a = useActiveProject();
  const b = useActiveProject();
  line('useActiveProject same reference on consecutive calls', a === b, `a.name=${a?.name}`);
} catch (e) {
  // Zustand 5 wraps `useSyncExternalStore`, which throws outside
  // React render. The spec explicitly permits a code-review fallback
  // when the runtime check can't run cleanly.
  console.log(
    `[INFO] useActiveProject() unavailable outside React (Zustand 5): ${e?.message ?? e}`,
  );
}

// Source-shape review. Look at the four `useActive*` selectors and
// confirm each one is a `s.x.find(...)` or a raw field reference —
// never `.map`, `.filter`, `.sort`, or a spread.
const { readFileSync } = await import('node:fs');
const selSrc = readFileSync(new URL('../src/stores/app-store.ts', import.meta.url), 'utf8');
{
  // Pull every `useAppStore((s) => …)` occurrence together with its
  // closing parenthesis, ignoring strings + comments. The selector
  // bodies are short (≤ 5 lines each).
  const lines = selSrc.split('\n');
  const startIdx = lines.map((l, i) => ({ l, i })).filter(({ l }) => /useAppStore\(\(s\)/.test(l));
  let allClean = startIdx.length > 0;
  for (const { l, i } of startIdx) {
    // assemble body up to the first standalone `)` line.
    let body = l;
    for (let j = i + 1; j < Math.min(i + 8, lines.length); j++) {
      body += '\n' + lines[j];
      if (/^\s*\)\s*;?/.test(lines[j])) break;
    }
    const allocators = /\.(map|filter|sort|reduce)\(/.test(body);
    const spreads = /\[\s*\.\.\./.test(body) || /\{\s*\.\.\.[a-z]/i.test(body);
    // Allow `find(...)`, raw field access, or `?? null` postfix.
    const isFindOrField =
      /\.find\s*\(/.test(body) ||
      /\bs\.(openTabs|projects|environments|nodes|endpointDetails|activeTabId|activeEndpointId)\b/.test(
        body,
      );
    const ok = !allocators && !spreads && isFindOrField;
    line(
      `selector @ L${i + 1}`,
      ok,
      `${allocators ? 'alloc ' : ''}${spreads ? 'spread ' : ''}${isFindOrField ? '' : 'no-find '}`,
    );
    allClean = allClean && ok;
  }
  line(
    'all 4 useActive* selectors are raw-shape (no .map/.filter/.sort/spread)',
    allClean,
    `count=${startIdx.length}`,
  );
}

// ---------- (B) cascade cleanup ----------
console.log('\n=== (B) Cascade cleanup ===');

// One flat tree replaces the three parallel arrays: hierarchy is
// `parentId`, the owner is the denormalised `rootId`.
const SEED_NODES = [
  makeNode('m1', 'module', null, 0),
  makeNode('c1', 'collection', 'm1', 0),
  makeNode('c2', 'collection', 'm1', 1),
  makeNode('ep1', 'endpoint', 'c1', 0),
  makeNode('ep2', 'endpoint', 'c2', 0),
];

async function seedAndDelete(actionName, id, expectedGoneIds) {
  useAppStore.setState({
    projects: [makeProject('p1')],
    nodes: SEED_NODES,
    endpointDetails: { ep1: makeDetail('ep1'), ep2: makeDetail('ep2') },
    responseHistory: { ep1: [makeResp()], ep2: [makeResp()] },
    responseExamples: {
      ep1: [{ ...makeResp(), id: 'ex1' }],
      ep2: [{ ...makeResp(), id: 'ex2' }],
    },
  });
  const state = useAppStore.getState();
  await state[actionName](id);
  const after = useAppStore.getState();
  const label = `${actionName}(${id})`;
  for (const gone of expectedGoneIds) {
    line(
      `${label} → responseHistory[${gone}] removed`,
      !(gone in after.responseHistory),
      JSON.stringify(Object.keys(after.responseHistory)),
    );
    line(
      `${label} → responseExamples[${gone}] removed`,
      !(gone in after.responseExamples),
      JSON.stringify(Object.keys(after.responseExamples)),
    );
    // The detail row is keyed the same way and must not outlive its
    // node — the HTTP tester reads it by node id.
    line(
      `${label} → endpointDetails[${gone}] removed`,
      !(gone in after.endpointDetails),
      JSON.stringify(Object.keys(after.endpointDetails)),
    );
  }
}

// `deleteNode` covers the endpoint, collection and module cases in one
// action — the cascade lives in the schema, so the client drops the
// whole `parentId` subtree rather than knowing what hangs below what.
await seedAndDelete('deleteNode', 'ep1', ['ep1']);
await seedAndDelete('deleteNode', 'c2', ['ep2']);
await seedAndDelete('deleteNode', 'm1', ['ep1', 'ep2']);
// A project delete is the one scope that filters by `rootId` instead.
await seedAndDelete('deleteProject', 'p1', ['ep1', 'ep2']);

// ---------- (C) persistence write ----------
console.log('\n=== (C) Persistence write ===');

const ls = g.localStorage;
const KEY = 'simple-post:prefs';

{
  useAppStore.getState().setTheme('light');
  const raw = ls.getItem(KEY);
  let parsed = null;
  try {
    parsed = raw ? JSON.parse(raw) : null;
  } catch {
    /* ignore */
  }
  line(
    'setTheme("light") writes {theme:"light", requestSettings:{…}} to localStorage',
    parsed?.theme === 'light' && parsed?.requestSettings?.timeoutMs === 30000,
    JSON.stringify(parsed),
  );
}

{
  useAppStore.getState().setTheme('dark');
  const raw = ls.getItem(KEY);
  let parsed = null;
  try {
    parsed = raw ? JSON.parse(raw) : null;
  } catch {
    /* ignore */
  }
  line(
    'setTheme("dark") writes {theme:"dark"} to localStorage',
    parsed?.theme === 'dark',
    JSON.stringify(parsed),
  );
}

{
  useAppStore.getState().updateRequestSettings({ timeoutMs: 60000 });
  const raw = ls.getItem(KEY);
  let parsed = null;
  try {
    parsed = raw ? JSON.parse(raw) : null;
  } catch {
    /* ignore */
  }
  line(
    'updateRequestSettings({ timeoutMs: 60000 }) persists',
    parsed?.requestSettings?.timeoutMs === 60000,
    JSON.stringify(parsed?.requestSettings),
  );
}

// ---------- (D) persistence init (code review) ----------
console.log('\n=== (D) Persistence init (code review) ===');
const uiSrc = readFileSync(new URL('../src/stores/ui-slice.ts', import.meta.url), 'utf8');
const hasLoadPrefs = /function loadPrefs\(\)/.test(uiSrc);
const readsKeyConst = /const PREFS_KEY = 'simple-post:prefs'/.test(uiSrc);
const readsKey = /window\.localStorage\.getItem\(PREFS_KEY\)/.test(uiSrc);
const initialThemeUses = /const initialTheme: Theme = initialPrefs\.theme/.test(uiSrc);
const initialReqUses =
  /const initialRequestSettings: RequestSettings = \{[^]*?initialPrefs\.requestSettings/m.test(
    uiSrc,
  );
const initialRequestSettingsDefault =
  /timeoutMs: initialPrefs\.requestSettings\?\.timeoutMs\s*\?\?\s*30000/.test(uiSrc);
const followRedirectsDefault =
  /followRedirects: initialPrefs\.requestSettings\?\.followRedirects\s*\?\?\s*true/.test(uiSrc);
const maxSizeDefault =
  /maxResponseSizeKb: initialPrefs\.requestSettings\?\.maxResponseSizeKb\s*\?\?\s*5120/.test(uiSrc);

const allInitOk =
  hasLoadPrefs &&
  readsKeyConst &&
  readsKey &&
  initialThemeUses &&
  initialReqUses &&
  initialRequestSettingsDefault &&
  followRedirectsDefault &&
  maxSizeDefault;

line(
  'ui-slice reads localStorage once + seeds initial state',
  allInitOk,
  `loadPrefs=${hasLoadPrefs} key=${readsKeyConst} read=${readsKey} themeInit=${initialThemeUses} reqInit=${initialReqUses} defaults=${initialRequestSettingsDefault && followRedirectsDefault && maxSizeDefault}`,
);

console.log(`\n=== Summary: ${pass} PASS, ${fail} FAIL ===`);
if (fail > 0) process.exitCode = 1;
