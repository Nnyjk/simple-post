/**
 * SettingsPanel — content of the (singleton) settings tab.
 *
 * Layout: a narrow left rail with the four category buttons and a
 * scrollable main area for the active section. The tab itself lives
 * in the workspace tab strip (kind: 'settings', id: '__settings__'),
 * so this component is just the body — no drawer chrome, no header,
 * no X-to-close (use the tab's X).
 *
 * MOUNTED TWICE, ONE IMPLEMENTATION
 * ---------------------------------
 * The workspace renders it inside the tab strip, and the project-less
 * `SettingsSurface` renders the very same component full-window for the
 * welcome screen. That is safe because everything below reads from the
 * store instead of taking props: the rail selection, the section data,
 * even which project's environments are in scope. The two hosts differ
 * only in the chrome around this body. See `SettingsSurface.tsx`.
 *
 * The right pane opens with a slim breadcrumb + section title so the
 * user always has a "where am I" cue without us repeating the rail label
 * twice on the same screen. Each sub-section renders its own header via
 * `SettingsCategory`, which in turn wraps rows in a soft card shell.
 */

import { useMemo, useRef } from 'react';
import { ChevronRight, Cloud, Cpu, Palette } from 'lucide-react';
import { Send } from 'lucide-react';
import { useAppStore, type SettingsCategoryId } from '@/stores/app-store';
import { EnvironmentSection } from './sections/EnvironmentSection';
import { ConnectionSection } from './sections/ConnectionSection';
import { AppearanceSection } from './sections/AppearanceSection';
import { RequestSection } from './sections/RequestSection';
import { cn } from '@/lib/utils';
import type { HueConcept } from '@/lib/hue';
import { IconTile } from '@/components/ui/icon-tile';
import { EmptyHint } from '@/components/ui/empty-hint';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';

const CATEGORY_IDS: readonly SettingsCategoryId[] = ['env', 'connection', 'appearance', 'request'];

const RAIL_ITEMS: {
  id: SettingsCategoryId;
  label: string;
  icon: React.ReactNode;
  hint: string;
  /**
   * Semantic hue for the rail pill, so the active row's icon tint reads
   * as the same concept the section header uses. Previously each entry
   * carried a raw Tailwind palette string typed by hand, which is how
   * `connection` ended up violet — the same colour as `project`.
   *
   * `env` and `appearance` take no hue: environment is the app's primary
   * context (`--primary`) and appearance is cosmetic, so both stay
   * neutral rather than inventing a colour with no informational gain.
   * See `src/lib/hue.ts`.
   */
  concept?: HueConcept;
}[] = [
  {
    id: 'env',
    label: '环境',
    icon: <Cloud className="h-4 w-4" />,
    hint: '环境变量 / baseUrl',
  },
  {
    id: 'connection',
    label: '连接',
    icon: <Cpu className="h-4 w-4" />,
    hint: 'MCP 协议设置',
    concept: 'connection',
  },
  {
    id: 'appearance',
    label: '外观与体验',
    icon: <Palette className="h-4 w-4" />,
    hint: '主题 / 快捷键',
  },
  {
    id: 'request',
    label: '请求',
    icon: <Send className="h-4 w-4" />,
    hint: '超时 / 重定向 / 体积上限',
    concept: 'request',
  },
];

const SECTION_META: Record<
  SettingsCategoryId,
  { title: string; description: string; icon: React.ReactNode }
> = {
  env: {
    title: '环境',
    description: '使用 {{var}} 引用，支持 query / header / body / url',
    icon: <Cloud className="h-3.5 w-3.5" />,
  },
  connection: {
    title: '连接',
    description: 'stdio 模式 · 供 Claude Desktop、Cursor 等调用',
    icon: <Cpu className="h-3.5 w-3.5" />,
  },
  appearance: {
    title: '外观与体验',
    description: '主题、快捷键等全局偏好',
    icon: <Palette className="h-3.5 w-3.5" />,
  },
  request: {
    title: '请求',
    description: '应用到所有 HTTP 发送（v0.2 由 Rust 端消费）',
    icon: <Send className="h-3.5 w-3.5" />,
  },
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
  const renameEnvironmentVar = useAppStore((s) => s.renameEnvironmentVar);
  const setEnvironmentModuleBaseUrl = useAppStore((s) => s.setEnvironmentModuleBaseUrl);
  const deleteEnvironment = useAppStore((s) => s.deleteEnvironment);
  const updateEnvironment = useAppStore((s) => s.updateEnvironment);
  const addEnvironment = useAppStore((s) => s.addEnvironment);

  // The section also manages baseUrl *definitions* (the project's named
  // services). Those actions touch the project, not the env.
  const projects = useAppStore((s) => s.projects);
  const addBaseUrlDefinition = useAppStore((s) => s.addBaseUrlDefinition);
  const updateBaseUrlDefinition = useAppStore((s) => s.updateBaseUrlDefinition);
  const deleteBaseUrlDefinition = useAppStore((s) => s.deleteBaseUrlDefinition);
  const setDefaultBaseUrlDefinition = useAppStore((s) => s.setDefaultBaseUrlDefinition);
  const activeProject = useMemo(
    () => projects.find((p) => p.id === activeProjectId) ?? null,
    [projects, activeProjectId],
  );

  const environments = useMemo(
    () => allEnvironments.filter((e) => e.projectId === activeProjectId),
    [allEnvironments, activeProjectId],
  );

  // Modules are the row axis of the 前置 URL table — the row set is
  // (every module × every project service) — so they are filtered to this
  // project. Sorted by `sortOrder` because the table presents them in tree
  // order and `nodes` is a flat list whose array order is not the tree's
  // sibling order.
  const nodes = useAppStore((s) => s.nodes);
  const modules = useMemo(
    () =>
      nodes
        .filter((n) => n.kind === 'module' && n.rootId === activeProjectId)
        .sort((a, b) => a.sortOrder - b.sortOrder),
    [nodes, activeProjectId],
  );

  const mainRef = useRef<HTMLElement>(null);

  // Cross-fade the active section when `activeSection` changes. The outer
  // wrapper carries `key={activeSection}` so React remounts the section on
  // each switch, and we then animate the freshly mounted root from a
  // faded-out + slightly-down state to its natural position.
  //
  // Purely decorative: the section remounts either way, so under reduced
  // motion it just appears. No end-state callback is needed — there is no
  // state behind the tween, unlike the tree's expand height or the
  // response panel's count-ups.
  useGSAP(
    () => {
      const root = mainRef.current?.querySelector<HTMLElement>('[data-settings-section]');
      if (!root) return;
      return motionSafe(() => {
        gsap.fromTo(
          root,
          { opacity: 0, y: 4 },
          { opacity: 1, y: 0, duration: 0.2, ease: 'power2.out' },
        );
      });
    },
    { scope: mainRef, dependencies: [activeSection] },
  );

  const meta = SECTION_META[activeSection];

  return (
    <div className="flex h-full min-h-0">
      {/* Left rail — vertical list of section entries. Mirrors the
          visual weight of the project tree so the settings tab feels
          like another navigation context, not a popup. */}
      <aside className="flex w-48 shrink-0 flex-col gap-0.5 border-r border-border bg-card/40 p-2">
        <div className="label-caps px-2 pb-2 pt-1 text-muted-foreground/70">分组</div>
        <nav className="flex flex-col gap-0.5" role="tablist" aria-label="设置分组">
          {RAIL_ITEMS.map((item) => {
            const active = item.id === activeSection;
            return (
              <button
                type="button"
                key={item.id}
                role="tab"
                aria-selected={active}
                onClick={() => setActiveSection(item.id)}
                className={cn(
                  'group flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors',
                  active
                    ? 'bg-accent text-foreground shadow-[inset_0_0_0_1px_hsl(var(--border))]'
                    : 'text-muted-foreground hover:bg-accent/60 hover:text-foreground',
                )}
              >
                <IconTile
                  size="xs"
                  icon={item.icon}
                  className={cn(
                    'transition-colors',
                    !active && 'bg-muted/60 text-muted-foreground group-hover:text-foreground',
                  )}
                  concept={active ? item.concept : undefined}
                />
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-medium">{item.label}</span>
                  <span
                    className={cn(
                      'block truncate text-[10px]',
                      active ? 'text-muted-foreground' : 'text-muted-foreground/70',
                    )}
                  >
                    {item.hint}
                  </span>
                </span>
              </button>
            );
          })}
        </nav>

        {/* Bottom-pinned helper card — surfaces where to file feedback. */}
        <div className="mt-auto rounded-md border border-dashed border-border/70 bg-background/30 px-2.5 py-2 text-[11px] leading-relaxed text-muted-foreground">
          设置改动会立即保存。重启 MCP 进程后部分项生效。
        </div>
      </aside>

      {/* Main content area. A slim breadcrumb + section title head the
          scroll area so the user always has a stable "where am I" cue
          even after the section body has scrolled. */}
      <main
        ref={mainRef}
        className="min-w-0 flex-1 overflow-y-auto"
        data-settings-active={activeSection}
      >
        <div className="flex items-center gap-1.5 px-12 pb-1 pt-5 text-[11px] text-muted-foreground">
          <span>设置</span>
          <ChevronRight className="h-3 w-3 text-muted-foreground/60" />
          <span className="text-foreground">{meta.title}</span>
        </div>
        {CATEGORY_IDS.map((id) => {
          if (id !== activeSection) return null;
          if (id === 'env') {
            // Environments belong to a project, and this panel is also
            // mounted on the project-less surface (`SettingsSurface.tsx`
            // — reachable from the welcome screen). Without this guard the
            // section rendered an empty env list *with* a working-looking
            // "新建环境" button whose write went to `projectId === ''` and
            // silently failed. Say what is missing instead of offering a
            // control that cannot succeed.
            if (activeProjectId === '') {
              return (
                <div key={activeSection} data-settings-section className="px-12 pb-4">
                  <EmptyHint className="px-3 py-10 text-xs">
                    环境变量属于项目。请先在欢迎页选择一个项目，再回到这里管理它的环境与前置 URL。
                  </EmptyHint>
                </div>
              );
            }
            return (
              <div key={activeSection} data-settings-section>
                <EnvironmentSection
                  environments={environments}
                  projectId={activeProjectId}
                  modules={modules}
                  setEnvironmentVar={setEnvironmentVar}
                  renameEnvironmentVar={renameEnvironmentVar}
                  setEnvironmentModuleBaseUrl={setEnvironmentModuleBaseUrl}
                  deleteEnvironment={deleteEnvironment}
                  updateEnvironment={updateEnvironment}
                  addEnvironment={addEnvironment}
                  baseUrlDefinitions={activeProject?.baseUrlDefinitions ?? []}
                  defaultBaseUrlDefinitionId={activeProject?.defaultBaseUrlDefinitionId ?? null}
                  addBaseUrlDefinition={addBaseUrlDefinition}
                  updateBaseUrlDefinition={updateBaseUrlDefinition}
                  deleteBaseUrlDefinition={deleteBaseUrlDefinition}
                  setDefaultBaseUrlDefinition={setDefaultBaseUrlDefinition}
                />
              </div>
            );
          }
          if (id === 'connection') {
            return (
              <div key={activeSection} data-settings-section>
                <ConnectionSection />
              </div>
            );
          }
          if (id === 'appearance') {
            return (
              <div key={activeSection} data-settings-section>
                <AppearanceSection />
              </div>
            );
          }
          // request
          return (
            <div key={activeSection} data-settings-section>
              <RequestSection />
            </div>
          );
        })}
      </main>
    </div>
  );
}
