/**
 * SettingsSurface —— 设置面板的**无项目**宿主。
 *
 * 为什么需要第二个宿主
 * -----------------
 * 设置是全局的：四个分区中的三个（连接 / 外观与体验 / 请求）不按项目划分，第四个（环境）
 * 则是应用唯一能诚实说「先选一个项目」的地方。若只能作为*标签*触达，欢迎页就够不到它 ——
 * 那里的按钮调用 `openSettingsTab()`，store 老老实实加了个标签，而 `App.tsx` 因为界面切换
 * 以 `activeProjectId` 为准，仍在渲染 `WelcomeScreen`，点击完全没反应。
 *
 * 修法不是第二份实现，而是第二个*挂载点*：
 *
 *   WelcomeScreen  ─┐
 *                   ├─→ SettingsPanel（一个文件、一份实现）
 *   WorkspaceShell ─┘        ↑
 *                             └ SettingsSurface（本文件）补上一个独立界面所需的 chrome
 *
 * `App.tsx` 既按 `activeProjectId === ''` 也按设置标签是否为当前标签来挑选界面，故两个宿主
 * 渲染的是同一份面板状态（`activeSettingsSection` 住在 store 里）。在项目内，工作区仍以标签
 * 的形式持有它 —— 那种位置自带标签条的关闭便利性；项目外没有标签条，故由这个表头提供返回
 * 的路。
 *
 * 为何不在这里复制一份面板：每个分区都从 store 而非 props 读数据，侧栏选中项也是 store 状态，
 * 故把它挂到两处不可能让同一个分区出现两种视图。唯一不同的是它周围的 chrome。
 */

import { ArrowLeft, Settings2 } from 'lucide-react';
import { useAppStore, SETTINGS_TAB_ID } from '@/stores/app-store';
import { Button } from '@/components/ui/button';
import { WindowControls } from '@/components/ui/window-controls';
import { SettingsPanel } from './SettingsPanel';

// 与顶栏同一个 Tauri 2 拖拽区退出写法：表头行开启拖拽，其中的每个控件都必须显式退出，
// 否则 webview 的拖拽手势会吞掉点击。
const NO_DRAG = { 'data-tauri-drag-region': 'false' } as const;

export function SettingsSurface() {
  // 设置永远不会是脏的（标签条只标记实体标签），故走的是普通关闭路径 —— 仍经由
  // `requestCloseTab`，这样万一情况变了也只有一条代码路径。
  const requestCloseTab = useAppStore((s) => s.requestCloseTab);

  return (
    <div
      data-testid="settings-surface"
      className="flex h-full min-h-0 w-full flex-col overflow-hidden bg-background"
    >
      {/* 窗口无边框（`decorations: false`），故这个界面需要自己的拖拽区与窗口控件，理由与
          欢迎页完全相同。面板本身不含 chrome —— 它本是写在标签里用的。 */}
      <header
        data-tauri-drag-region="true"
        className="flex h-11 shrink-0 select-none items-center gap-2 border-b border-border bg-card/50 px-3"
      >
        <Button
          {...NO_DRAG}
          variant="ghost"
          size="xs"
          onClick={() => requestCloseTab(SETTINGS_TAB_ID)}
          aria-label="返回"
          className="text-muted-foreground hover:text-foreground"
        >
          <ArrowLeft className="h-3.5 w-3.5" />
          返回
        </Button>
        <span className="flex items-center gap-1.5 text-sm font-semibold tracking-tight">
          <Settings2 className="h-4 w-4 text-muted-foreground" />
          设置
        </span>
        <div className="ml-auto flex items-center">
          <WindowControls />
        </div>
      </header>

      <div className="min-h-0 flex-1">
        <SettingsPanel />
      </div>
    </div>
  );
}
