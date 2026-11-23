/**
 * store 扩展独立验证脚本：无需启动浏览器或 Tauri 运行时。
 *
 * 验证目标与防范风险：
 *   (A) 选择器稳定性：防范选择器在状态未变时产生新引用（严禁 map/filter/spread 等分配开销导致 React 意外重渲）。
 *   (B) 级联清理：防范删除节点后残留历史、示例与详情孤儿数据（覆盖 endpoint/collection/module/project 四级）。
 *   (C) 持久化写入：防范主题未写入 localStorage 或请求旋钮误写入 localStorage（请求旋钮必须落后端 preferences 表）。
 *   (D) 持久化初始化：防范非法主题注入 store 以及默认请求旋钮偏离后端真值。
 *
 * IPC 桩说明：
 *   delete* 采用乐观更新并在 catch 中按快照回滚；本脚本让 IPC 返回 null 模拟成功路径，验证本地状态级联清理逻辑。
 */

// ── 浏览器与 IPC 桩环境 ──────────────────────────────────────────────────
import { mockIPC } from '@tauri-apps/api/mocks';

const g = globalThis;
if (!g.window) g.window = g;
if (!g.window.__TAURI_INTERNALS__) g.window.__TAURI_INTERNALS__ = {};

// Node 22+ 原生 localStorage 无方法，替换为 Map 桩以满足 getItem/setItem/removeItem 规范。
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

// 模拟 IPC 成功返回以保留级联修改，并记录调用参数供持久化断言校验。
const ipcCalls = [];
mockIPC(async (cmd, args) => {
  ipcCalls.push({ cmd, args });
  return null;
});

// ---------- 优先导入共享模块 ----------
// 首帧脚本与 store 均由此读取唯一 PREFS_KEY，须在 store 求值前写入 localStorage。
const { default: themeBoot } = await import('../src/lib/theme-boot/index.ts');
const KEY = themeBoot.PREFS_KEY;

// 预置非法主题 'blue'：防范 store 忽略解析器校验直接使用存储值（合法回退为 'dark'）。
g.localStorage.setItem(KEY, JSON.stringify({ theme: 'blue' }));

// ── 导入 store 模块 ───────────────────────────────────────────────────────
const { useAppStore, useActiveProject } = await import('../src/stores/app-store.ts');

// ---------- store 初始状态 ----------
// 导入后立即记录初始状态，用于断言初始化行为，防止被后续测试修改。
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
    color: '#000000',
    createdAt: 0,
    updatedAt: 0,
  };
}
function makeNode(id, kind, parentId, sortOrder) {
  // 字段为 kind 而非 type；防范因字段名过时导致过滤静默失效。
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
// 构建包含完整外层的 ResponseHistoryEntry，防范历史数据结构回归。
function makeHistoryEntry(id, endpointId) {
  return { id, endpointId, response: makeResp(), bodyTruncated: false, createdAt: 0 };
}

// ── (A) 状态选择器引用稳定性验证 ──────────────────────────────────────────
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
  // Zustand 5 的 useSyncExternalStore 在 React 渲染树外抛错时降级为源码静态检查。
  console.log(
    `[INFO] useActiveProject() unavailable outside React (Zustand 5): ${e?.message ?? e}`,
  );
}

// 静态源码检查：防范 useActive* 选择器内部使用 map/filter/sort/spread 等导致新引用的分配操作。
const { readFileSync } = await import('node:fs');
const selSrc = readFileSync(new URL('../src/stores/app-store.ts', import.meta.url), 'utf8');
{
  // 提取各 useAppStore((s) => …) 选择器闭包体进行语法特征扫描。
  const lines = selSrc.split('\n');
  const startIdx = lines.map((l, i) => ({ l, i })).filter(({ l }) => /useAppStore\(\(s\)/.test(l));
  let allClean = startIdx.length > 0;
  for (const { l, i } of startIdx) {
    // 拼接函数体直至遇到首个独立的 ')' 行。
    let body = l;
    for (let j = i + 1; j < Math.min(i + 8, lines.length); j++) {
      body += '\n' + lines[j];
      if (/^\s*\)\s*;?/.test(lines[j])) break;
    }
    const allocators = /\.(map|filter|sort|reduce)\(/.test(body);
    const spreads = /\[\s*\.\.\./.test(body) || /\{\s*\.\.\.[a-z]/i.test(body);
    // 仅允许 find(...)、直接字段访问或 '?? null' 后缀。
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

// ── (B) 级联清理孤儿数据验证 ──────────────────────────────────────────────
console.log('\n=== (B) Cascade cleanup ===');

// 树节点共用单表：通过 parentId 表达层级，通过反范式化 rootId 关联所属项目。
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
    // 防范详情数据脱离节点残留孤儿行（HTTP 测试器按 nodeId 读取）。
    line(
      `${label} → endpointDetails[${gone}] removed`,
      !(gone in after.endpointDetails),
      JSON.stringify(Object.keys(after.endpointDetails)),
    );
  }
}

// deleteNode 统一处理端点、集合与模块：客户端级联清理整棵 parentId 子树。
await seedAndDelete('deleteNode', 'ep1', ['ep1']);
await seedAndDelete('deleteNode', 'c2', ['ep2']);
await seedAndDelete('deleteNode', 'm1', ['ep1', 'ep2']);
// deleteProject 作用域按 rootId 匹配清理整个项目下的端点关联数据。
await seedAndDelete('deleteProject', 'p1', ['ep1', 'ep2']);

// ── (C) 持久化存储写入验证 ────────────────────────────────────────────────
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
  // 请求旋钮归属于后端 preferences 表（MCP 亦由此读取），断言 store 发往后端的 IPC 命令。
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

// ── (D) 持久化配置初始化验证 ──────────────────────────────────────────────
console.log('\n=== (D) Persistence init ===');
// 通过实际运行时行为断言 store 初始化逻辑，避免正则匹配代码文本导致误报。
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
