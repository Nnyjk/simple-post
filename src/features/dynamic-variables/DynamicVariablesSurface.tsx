/**
 * 无项目时承载 `DynamicVariablesPanel` 的界面；`App.tsx` 按 `DYNAMIC_TAB_ID` 选择它。
 * 无标签条，故由本组件的表头提供「返回」。
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
