import type { StateCreator } from 'zustand';
import themeBoot from '@/lib/theme-boot';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { AppState } from '../app-store';
import type { RequestSettings, Theme } from '../store-types';

/**
 * ui-prefs —— 主题与出站请求旋钮。
 *
 * 两者的存法不同，且是有意的：
 *
 * - **主题**只在渲染层有意义，故留在 localStorage。
 * - **请求旋钮**（超时 / 跟随重定向 / 最大响应体积）决定每一次外发请求的形态，故存在
 *   `preferences` 表的 `request_settings` 行里 —— 那是后端读得到的地方。MCP 侧发请求时
 *   读的是同一行，故界面上把超时调成 5 秒，AI 客户端发出去也是 5 秒；旋钮只落在
 *   localStorage 的话，后端只知道一个写死的默认值。
 *
 * localStorage 助手保持模块私有：store 之外不该写这个 key。**读**的那一半与「什么算合法
 * 主题」在 `src/lib/theme-boot/index.ts`——那里同时被 `index.html` 的首帧脚本与界面读，
 * 写不出第二份去。
 */

/**
 * store 读到的偏好。
 *
 * `theme` 存的是 localStorage 里的原始取值，**未经校验**：用户可以手改 localStorage，
 * 旧版本也可能留下别的名字。合不合法由 `themeBoot.resolveTheme` 判定。
 */
interface StorePrefs {
  theme?: string;
  lastProjectId?: string;
  requestSettings?: Partial<RequestSettings>;
}

function loadPrefs(): StorePrefs {
  if (typeof window === 'undefined') return {};
  try {
    return themeBoot.parsePrefs(window.localStorage.getItem(themeBoot.PREFS_KEY)) as StorePrefs;
  } catch {
    return {};
  }
}

function savePrefs(prefs: StorePrefs): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(themeBoot.PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // localStorage 可能被禁用（隐私模式、配额）；静默忽略。
  }
}

const initialPrefs = loadPrefs();
/**
 * 初始主题。
 *
 * 存下来的取值先经 `resolveTheme` 校验，非法值（手改坏的、旧版本留下的）回落成缺省的
 * `dark`。**校验不能省**：少了它，store 会把非法取值当主题存下，而 `useIsDark` 只认
 * `dark` 与 `light`，其余一律按「跟系统」处理——于是首帧画深色、随后 React 画系统色。
 */
const initialTheme: Theme = themeBoot.resolveTheme(initialPrefs.theme);

/**
 * 读出上次停留的项目 id。
 *
 * 空串表示「上次不在任何项目里」，刷新后因此回到欢迎页。
 */
export function readLastProjectId(): string {
  return loadPrefs().lastProjectId ?? '';
}

/**
 * 记住用户当前停留的项目。
 *
 * 空串是「不在任何项目里」，故它删掉这个键而不是存一个空值——空值与「没记过」
 * 在读回来时无法区分。
 */
export function saveLastProjectId(id: string): void {
  const prefs = loadPrefs();
  if (id) {
    prefs.lastProjectId = id;
  } else {
    delete prefs.lastProjectId;
  }
  savePrefs(prefs);
}

/**
 * 请求旋钮的默认值。**与后端 `RequestSettings::default()` 相同**（超时 30 秒、
 * 跟随重定向、最大响应 5120 KB）—— 两侧的缺省值必须是同一个数，否则「没配置过时」
 * 界面与 MCP 侧会发出不同的请求。
 */
export const REQUEST_SETTINGS_DEFAULTS: RequestSettings = {
  timeoutMs: 30_000,
  followRedirects: true,
  maxResponseSizeKb: 5_120,
};

export interface UiPrefsState {
  theme: Theme;
  requestSettings: RequestSettings;
}

export interface UiPrefsActions {
  setTheme: (t: Theme) => void;
  updateRequestSettings: (patch: Partial<RequestSettings>) => void;
}

export type UiPrefs = UiPrefsState & UiPrefsActions;

/**
 * 读一次请求旋钮，并把旧版本留在 localStorage 里的那份一次性迁进库。
 *
 * 迁移只做一次：写库成功后删掉 localStorage 里的旧字段，故它不会在之后的一次启动里
 * 盖掉用户在界面上新选的值。写库失败时保留旧值，下次启动再迁一次。
 *
 * 返回值总是完整的三个字段：后端那一行可能只存了部分字段（`RequestSettings` 的
 * 每个字段都是 `#[serde(default)]`），缺的那几项用默认值补上。
 */
export async function loadRequestSettings(): Promise<RequestSettings> {
  const legacy = loadPrefs().requestSettings;
  if (legacy) {
    const next = { ...REQUEST_SETTINGS_DEFAULTS, ...legacy };
    try {
      await invokeCmd(COMMANDS.requestSettingsUpdate, { patch: next });
      const prefs = loadPrefs();
      delete prefs.requestSettings;
      savePrefs(prefs);
    } catch (e: unknown) {
      console.error('[store] request settings migration failed', e);
    }
  }
  const stored = await invokeCmd<RequestSettings>(COMMANDS.requestSettingsGet).catch(
    (e: unknown) => {
      console.warn('[store] request settings unavailable', e);
      return REQUEST_SETTINGS_DEFAULTS;
    },
  );
  return { ...REQUEST_SETTINGS_DEFAULTS, ...stored };
}

export const createUiPrefs: StateCreator<AppState, [], [], UiPrefs> = (set, get) => ({
  theme: initialTheme,
  requestSettings: REQUEST_SETTINGS_DEFAULTS,

  /**
   * 换主题。只写 localStorage —— 主题是渲染层的事，后端读不到也不需要。
   *
   * 合并而不是整份替换：这份偏好里还有「上次停留的项目」，整份替换会把它抹掉，
   * 于是换一次主题就把刷新后的落点退回欢迎页。
   */
  setTheme: (t) => {
    set({ theme: t });
    savePrefs({ ...loadPrefs(), theme: t });
  },
  /**
   * 改请求旋钮。本地副本立刻更新（界面不等 IPC），同时把**完整三项**写进后端
   * —— web 形态的 `/api/request-settings` 是全量替换语义，发补丁会丢掉另外两项。
   */
  updateRequestSettings: (patch) => {
    const next = { ...get().requestSettings, ...patch };
    set({ requestSettings: next });
    // 写库。web 形态的 `/api/request-settings` 是**全量替换**语义，故这里发的是合并后
    // 的完整三项而不是补丁 —— 两种传输收到的都是同一份完整取值。
    void invokeCmd(COMMANDS.requestSettingsUpdate, { patch: next }).catch((e: unknown) => {
      console.error('[store] request settings update failed', e);
    });
  },
});
