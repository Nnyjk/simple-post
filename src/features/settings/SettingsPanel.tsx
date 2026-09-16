/**
 * SettingsPanel — content of the (singleton) settings tab.
 *
 * Layout: a narrow left rail with the four category buttons and a
 * scrollable main area for the active section. The tab itself lives
 * in the workspace tab strip (kind: 'settings', id: '__settings__'),
 * so this component is just the body — no drawer chrome, no header,
 * no X-to-close (use the tab's X).
 *
 * Why a left rail instead of the previous top tab strip?
 *   - Categories are stable (4 of them, not growing fast) so a vertical
 *     list fits without scrolling.
 *   - The left rail keeps a stable "where am I" cue while the right side
 *     scrolls long forms.
 *   - It frees the top edge for a section title + description.
 */

import { useMemo } from 'react';
import { Cloud, Cpu, Palette, Send } from 'lucide-react';
import { useAppStore, type SettingsCategoryId } from '@/stores/app-store';
import { SettingsCategory } from './SettingsCategory';
import { EnvironmentSection } from './sections/EnvironmentSection';
import { ConnectionSection } from './sections/ConnectionSection';
import { AppearanceSection } from './sections/AppearanceSection';
import { RequestSection } from './sections/RequestSection';
import { cn } from '@/lib/utils';

const CATEGORY_IDS: readonly SettingsCategoryId[] = [
  'env',
  'connection',
  'appearance',
  'request',
];

const RAIL_ITEMS: { id: SettingsCategoryId; label: string; icon: React.ReactNode; hint: string }[] = [
  { id: 'env', label: '环境', icon: <Cloud className="h-4 w-4" />, hint: '环境变量 / baseUrl' },
  { id: 'connection', label: '连接', icon: <Cpu className="h-4 w-4" />, hint: 'MCP 协议设置' },
  { id: 'appearance', label: '外观与体验', icon: <Palette className="h-4 w-4" />, hint: '主题 / 快捷键' },
  { id: 'request', label: '请求', icon: <Send className="h-4 w-4" />, hint: '超时 / 重定向 / 体积上限' },
];

const SECTION_META: Record<
  SettingsCategoryId,
  { title: string; description: string }
> = {
  env: { title: '环境', description: '使用 {{var}} 引用，支持 query / header / body / url' },
  connection: { title: '连接', description: 'stdio 模式 · 供 Claude Desktop、Cursor 等调用' },
  appearance: { title: '外观与体验', description: '主题、快捷键等全局偏好' },
  request: { title: '请求', description: '应用到所有 HTTP 发送（v0.2 由 Rust 端消费）' },
};

export function SettingsPanel() {
  // The active section lives in the store so a deep link from the
  // TopBar env switcher (which calls `openSettingsTab('env')`) lands
  // on the right section the first time the tab opens.
  const activeSection = useAppStore((s) => s.activeSettingsSection);
  const setActiveSection = useAppStore((s) => s.setActiveSettingsSection);

  // The env section needs to filter environments down to the active
  // project. Pull the data here so the section component stays
  // presentational.
  const allEnvironments = useAppStore((s) => s.environments);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const setEnvironmentVar = useAppStore((s) => s.setEnvironmentVar);
  const addEnvironment = useAppStore((s) => s.addEnvironment);
  const environments = useMemo(
    () => allEnvironments.filter((e) => e.projectId === activeProjectId),
    [allEnvironments, activeProjectId],
  );

  return (
    <div className="flex h-full min-h-0">
      {/* Left rail — vertical list of section entries. Mirrors the
          visual weight of the project tree so the settings tab feels
          like another navigation context, not a popup. */}
      <aside className="flex w-44 shrink-0 flex-col gap-0.5 border-r border-border bg-card/40 p-2">
        <div className="px-2 py-1.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
          分组
        </div>
        <nav className="flex flex-col gap-0.5" role="tablist" aria-label="设置分组">
          {RAIL_ITEMS.map((item) => {
            const active = item.id === activeSection;
            return (
              <button
                key={item.id}
                role="tab"
                aria-selected={active}
                onClick={() => setActiveSection(item.id)}
                className={cn(
                  'group flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors',
                  active
                    ? 'bg-primary/15 text-primary'
                    : 'text-muted-foreground hover:bg-accent/60 hover:text-foreground',
                )}
              >
                <span
                  className={cn(
                    'shrink-0',
                    active ? 'text-primary' : 'text-muted-foreground group-hover:text-foreground',
                  )}
                >
                  {item.icon}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-medium">{item.label}</span>
                  <span
                    className={cn(
                      'block truncate text-[10px]',
                      active ? 'text-primary/70' : 'text-muted-foreground/70',
                    )}
                  >
                    {item.hint}
                  </span>
                </span>
              </button>
            );
          })}
        </nav>
      </aside>

      {/* Main content area. The page-level title is intentionally
          omitted — the left rail already labels the section and
          repeating it would just be visual noise. Each sub-section
          (rendered by the section component below) uses
          SettingsCategory, which draws a `border-b` divider under
          itself; together they form a single stacked column separated
          by thin horizontal lines. */}
      <main className="min-w-0 flex-1 overflow-y-auto">
        {CATEGORY_IDS.map((id) => {
          if (id !== activeSection) return null;
          if (id === 'env') {
            return (
              <div key={id} className="pt-2">
                <EnvironmentSection
                  environments={environments}
                  setEnvironmentVar={setEnvironmentVar}
                  addEnvironment={addEnvironment}
                  projectId={activeProjectId}
                />
              </div>
            );
          }
          if (id === 'connection') {
            return <ConnectionSection key={id} />;
          }
          if (id === 'appearance') {
            return <AppearanceSection key={id} />;
          }
          // request
          return <RequestSection key={id} />;
        })}
      </main>
    </div>
  );
}
