/**
 * ui-slice 的公开接口。实现按关注点拆到 `./slices/`，在这里组合，
 * 不在 `app-store.ts` 里注册为独立 slice。
 *
 * 多个 action 刻意在同一次 `set()` 里写别的 slice 的键——`submitNewItem` 写
 * `lastCreatedId` 并调 `addProject` / `addNode` / `setActiveEndpoint`；`deleteNode`
 * 一起剪掉 `responseHistory` / `responseExamples` / `openTabs` / `endpointDrafts`；
 * `saveDraftEndpoint` 按顺序改 draft map、endpoint-draft map 和两份标签列表。
 * 拆成多次 `get().closeTab()` 式的调用会把一次 `set()` 变成多次，
 * IPC 失败时能回滚的状态也就不一样了。
 *
 * 拓扑是星形：子工厂可 import `../store-types` 与共享的 `AppState` 类型，
 * 彼此之间、以及与本文件之间都不互相 import。各取标准
 * `(set, get, store)` 三元组，只返回自己的 state + action 对象。
 */
import type { StateCreator } from 'zustand';
import type { AppState } from './app-store';
import { createUiTabs } from './slices/ui-tabs';
import type { UiTabs } from './slices/ui-tabs';
import { createUiResponse } from './slices/ui-response';
import type { UiResponse } from './slices/ui-response';
import { createUiPrefs } from './slices/ui-prefs';
import type { UiPrefs } from './slices/ui-prefs';
import { createUiNewItem } from './slices/ui-new-item';
import type { UiNewItem } from './slices/ui-new-item';
import { createUiDraftEndpoints } from './slices/ui-draft-endpoints';
import type { UiDraftEndpoints } from './slices/ui-draft-endpoints';

/** 组合后的 UI slice；每个成员在 `./slices/` 里紧挨实现处声明。 */
export interface UiSlice extends UiTabs, UiResponse, UiPrefs, UiNewItem, UiDraftEndpoints {}

export const createUiSlice: StateCreator<AppState, [], [], UiSlice> = (set, get, store) => ({
  ...createUiTabs(set, get, store),
  ...createUiResponse(set, get, store),
  ...createUiPrefs(set, get, store),
  ...createUiNewItem(set, get, store),
  ...createUiDraftEndpoints(set, get, store),
});
