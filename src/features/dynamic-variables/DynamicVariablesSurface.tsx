/**
 * DynamicVariablesSurface —— 动态变量面板的**无项目**宿主。
 *
 * 为什么需要第二个宿主
 * -----------------
 * 动态变量是全局的：一条定义不属于任何项目，任何项目的端点都能引用它。故用户在没有
 * 打开项目时也要能触达它——那正是「先把一条基础数据源定义好，再去各个项目里引用」的
 * 场景。若只能作为*标签*触达，欢迎页就够不到它：那里的按钮调用
 * `openDynamicVariablesTab()`，store 加了个标签，而 `App.tsx` 因为界面切换以
 * `activeProjectId` 为准，仍在渲染 `WelcomeScreen`，点击完全没反应。
 *
 * 修法不是第二份实现，而是第二个*挂载点*：
 *
 *   WelcomeScreen  ─┐
 *                   ├─→ DynamicVariablesPanel（一个文件、一份实现）
 *   WorkspaceShell ─┘        ↑
 *                             └ DynamicVariablesSurface（本文件）补上一个独立界面所需的 chrome
 *
 * `App.tsx` 既按 `activeProjectId === ''` 也按动态变量标签是否为当前标签来挑选界面。
 * 项目外没有标签条，故由这个表头提供返回的路。
 */

import { ArrowLeft, Braces } from 'lucide-react';
import { useAppStore, DYNAMIC_TAB_ID } from '@/stores/app-store';
import { Button } from '@/components/ui/button';
import { WindowControls } from '@/components/ui/window-controls';
import { DynamicVariablesPanel } from './DynamicVariablesPanel';

// 与顶栏同一个 Tauri 2 拖拽区退出写法：表头行开启拖拽，其中的每个控件都必须显式退出，
// 否则 webview 的拖拽手势会吞掉点击。
const NO_DRAG = { 'data-tauri-drag-region': 'false' } as const;

export function DynamicVariablesSurface() {
  // 动态变量永远不会是脏的（标签条只标记实体标签），故走的是普通关闭路径 —— 仍经由
  // `requestCloseTab`，这样万一情况变了也只有一条代码路径。
  const requestCloseTab = useAppStore((s) => s.requestCloseTab);

  return (
    <div
      data-testid="dynamic-variables-surface"
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
          onClick={() => requestCloseTab(DYNAMIC_TAB_ID)}
          aria-label="返回"
          className="text-muted-foreground hover:text-foreground"
        >
          <ArrowLeft className="h-3.5 w-3.5" />
          返回
        </Button>
        <span className="flex items-center gap-1.5 text-sm font-semibold tracking-tight">
          <Braces className="h-4 w-4 text-muted-foreground" />
          动态变量
        </span>
        <div className="ml-auto flex items-center">
          <WindowControls />
        </div>
      </header>

      <div className="min-h-0 flex-1">
        <DynamicVariablesPanel />
      </div>
    </div>
  );
}
