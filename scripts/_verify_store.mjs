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
 *   (C) Persistence — `setTheme` must persist to localStorage under the
 *       prefs key, and `updateRequestSettings` must send the request knobs
 *       to the **backend** (they live in the `preferences` table, not in
 *       localStorage). The init path is asserted by behaviour: the store
 *       seeds its state from localStorage when the module is evaluated, so
 *       the assertions compare what the store holds against what the
 *       shared module (`src/lib/theme-boot/index.ts`) resolves.
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
//
// The calls are captured so the persistence section can assert what the
// store sent to the backend: the request knobs live there, not in
// localStorage.
const ipcCalls = [];
mockIPC(async (cmd, args) => {
  ipcCalls.push({ cmd, args });
  return null;
});

// ---------- import the shared module first ----------
//
// The prefs key comes from the shared module — the one place it is written
// down. `index.html`'s first-frame script and the store both read it there.
//
// It is read *before* the store, because the store computes its initial
// state while its module is evaluated: whatever the store should see has
// to be in localStorage by then.
const { default: themeBoot } = await import('../src/lib/theme-boot/index.ts');
const KEY = themeBoot.PREFS_KEY;

// Seed a theme the shared resolver rejects. With an empty store the
// check below compares 'dark' with 'dark' and passes even if the store
// ignored the resolver; with a rejected value the store has to have run
// that value through it. The key comes from the shared module, so this
// harness does not write a second copy of it.
g.localStorage.setItem(KEY, JSON.stringify({ theme: 'blue' }));

// ---------- import the store ----------
const { useAppStore, useActiveProject } = await import('../src/stores/app-store.ts');

// ---------- the state the store starts with ----------
//
// Captured right after the import, before any section below mutates it:
// the persistence-init assertions are about the **initial** state, and the
// sections that follow change it.
const ls = g.localStorage;
const initialState = useAppStore.getState();
const initialPrefs = themeBoot.parsePrefs(ls.getItem(KEY));

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
function makeNode(id, kind, parentId, sortOrder) {
  // `kind`, not `type`: the one-`nodes`-table refactor renamed the field,
  // and both cascade paths filter on `n.kind === 'endpoint'` — a seed
  // still calling it `type` silently prunes nothing and reports the whole
  // section green about the wrong thing.
  return {
    id,
    rootId: 'p1',
    parentId,
    kind,
    name: id.toUpperCase(),
    description: '',
    sortOrder,
    createdAt: 0,
    updatedAt: 0,
  };
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
// One `response_history` row. The wrapper is what the strip reads; the
// assertions below only ask whether a *key* survives a cascade, but
// seeding a bare `ApiResponse` here would let a shape regression in
// `ResponseHistoryEntry` pass unnoticed in the one script that seeds it.
function makeHistoryEntry(id, endpointId) {
  return { id, endpointId, response: makeResp(), bodyTruncated: false, createdAt: 0 };
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
    responseHistory: {
      ep1: [makeHistoryEntry('rh1', 'ep1')],
      ep2: [makeHistoryEntry('rh2', 'ep2')],
    },
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
    'setTheme("light") writes {theme:"light"} to localStorage, and no request knobs',
    parsed?.theme === 'light' && parsed?.requestSettings === undefined,
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
  // The request knobs live in the backend's `preferences` table — that is
  // where MCP reads them from too. So this one asserts the *command* the
  // store sent, not what it wrote to localStorage.
  ipcCalls.length = 0;
  useAppStore.getState().updateRequestSettings({ timeoutMs: 60000 });
  const call = ipcCalls.find((c) => c.args?.patch?.timeoutMs === 60000);
  const raw = ls.getItem(KEY);
  let parsed = null;
  try {
    parsed = raw ? JSON.parse(raw) : null;
  } catch {
    /* ignore */
  }
  line(
    'updateRequestSettings({ timeoutMs: 60000 }) sends all three knobs to the backend',
    !!call &&
      call.args.patch.followRedirects === true &&
      call.args.patch.maxResponseSizeKb === 5120,
    JSON.stringify(call?.args?.patch ?? null),
  );
  line(
    'updateRequestSettings leaves the request knobs out of localStorage',
    parsed?.requestSettings === undefined,
    JSON.stringify(parsed),
  );
}

// ---------- (D) persistence init ----------
console.log('\n=== (D) Persistence init ===');
// The init path is asserted by **behaviour**, not by matching the source
// text: the store seeds its state from localStorage when the module is
// evaluated, and the shape of that code changes for unrelated reasons
// often enough that a regex on it goes stale without anyone noticing.
//
// The shared module is where the key and the theme rules live; the store
// and the first-frame script in `index.html` both read them from there.
{
  const state = initialState;
  const stored = initialPrefs;
  line(
    'the store seeds its theme through the shared resolver',
    state.theme === themeBoot.resolveTheme(stored.theme),
    `theme=${state.theme} stored=${JSON.stringify(stored.theme ?? null)}`,
  );
  line(
    'a stored theme the resolver rejects does not become the store theme',
    stored.theme === 'blue' && state.theme === 'dark',
    `theme=${state.theme} stored=${JSON.stringify(stored.theme ?? null)}`,
  );
  line(
    'an unrecognised stored theme falls back to dark',
    themeBoot.resolveTheme('blue') === 'dark' && themeBoot.resolveTheme(undefined) === 'dark',
    `resolveTheme('blue')=${themeBoot.resolveTheme('blue')}`,
  );
  line(
    "the store seeds the request knobs with the backend's defaults",
    state.requestSettings.timeoutMs === 30000 &&
      state.requestSettings.followRedirects === true &&
      state.requestSettings.maxResponseSizeKb === 5120,
    JSON.stringify(state.requestSettings),
  );
  line(
    'the prefs key is still the one the first-frame script reads',
    KEY === 'simple-post:prefs',
    KEY,
  );
}

console.log(`\n=== Summary: ${pass} PASS, ${fail} FAIL ===`);
if (fail > 0) process.exitCode = 1;
