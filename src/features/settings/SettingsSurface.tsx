/**
 * SettingsSurface — the *project-less* host for the settings panel.
 *
 * WHY A SECOND HOST
 * -----------------
 * Settings is global: three of its four sections (连接 / 外观与体验 / 请求)
 * are not scoped to a project, and the fourth (环境) is the one place the
 * app can honestly say "先选一个项目". It used to be reachable only as a
 * *tab*, which meant it was unreachable from the welcome screen — the
 * button there called `openSettingsTab()`, the store dutifully added a
 * tab, and `App.tsx` kept rendering `WelcomeScreen` because the surface
 * switch keys on `activeProjectId`, so the click did nothing at all.
 *
 * The fix is not a second implementation. It is a second *mount*:
 *
 *   WelcomeScreen  ─┐
 *                   ├─→ SettingsPanel (one file, one implementation)
 *   WorkspaceShell ─┘        ↑
 *                             └ SettingsSurface (this file) adds the
 *                               chrome a standalone surface needs
 *
 * `App.tsx` picks the surface on `activeProjectId === ''` **and** on
 * whether the settings tab is the active one, so both hosts render the
 * same panel state (`activeSettingsSection` lives in the store). Inside a
 * project the workspace still owns it as a tab, which is the placement
 * that carries the tab strip's own close affordance; outside a project
 * there is no tab strip, so this header supplies the way back.
 *
 * Why the panel is not duplicated here: every section reads its data from
 * the store rather than from props, and the rail selection is store state,
 * so mounting it in two places cannot produce two different views of the
 * same section. The only thing that differs is the chrome around it.
 */

import { ArrowLeft, Settings2 } from 'lucide-react';
import { useAppStore, SETTINGS_TAB_ID } from '@/stores/app-store';
import { Button } from '@/components/ui/button';
import { WindowControls } from '@/components/ui/window-controls';
import { SettingsPanel } from './SettingsPanel';

// Same Tauri 2 drag-region opt-out the top bar uses: the header row is
// drag-enabled, and every control inside it must explicitly opt out or
// the webview drag gesture swallows the click.
const NO_DRAG = { 'data-tauri-drag-region': 'false' } as const;

export function SettingsSurface() {
  // Settings is never dirty (the tab strip only marks entity tabs), so
  // the plain close path is taken — and going through `requestCloseTab`
  // anyway means one code path if that ever changes.
  const requestCloseTab = useAppStore((s) => s.requestCloseTab);

  return (
    <div
      data-testid="settings-surface"
      className="flex h-full min-h-0 w-full flex-col overflow-hidden bg-background"
    >
      {/* The window is undecorated (`decorations: false`), so this
          surface needs its own drag region + window controls for exactly
          the same reason the welcome screen has one. The panel itself is
          chrome-free — it was written to live inside a tab. */}
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
