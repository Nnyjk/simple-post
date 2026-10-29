/**
 * SettingsPanel —— （单例）设置标签的内容。
 *
 * 布局：左侧一条窄栏放四个分类按钮，右侧是可滚动的主区显示当前分区。标签本身住在
 * 工作区标签条里（kind: 'settings'，id: '__settings__'），故本组件只是正文 ——
 * 没有抽屉外框、没有页眉、没有关闭 X（用标签自己的 X）。
 *
 * 挂载两处，一份实现
 * ------------------
 * 工作区把它渲染进标签条，无项目的 `SettingsSurface` 则把同一个组件铺满整窗供欢迎页用。
 * 这是安全的，因为下面的一切都从 store 读而非吃 props：选中的分区、各分区数据，甚至
 * 哪些项目的环境在范围内。两个宿主只在这具正文外面的外框上有差别。见 `SettingsSurface.tsx`。
 *
 * 环境分区在这条链上还要再往下一层：本面板只决定显示哪个分区，该分区自己的 store 接线
 * 住在 `ProjectEnvSection`，因为*项目标签*也把它当作自己的一层挂载。两个宿主、一份分区，
 * 没有第二份「哪些环境和模块在范围内」。
 *
 * 右栏开头是一段细窄的面包屑 + 分区标题，让用户始终有「我在哪」的提示，而不必在同一屏
 * 上把栏上的标签重复两遍。每个子分区经 `SettingsCategory` 渲染自己的头，`SettingsCategory`
 * 又把行包进一层柔和的卡片外壳。
 */

import { useRef } from 'react';
import { ChevronRight, Cloud, Cpu, Palette } from 'lucide-react';
import { Send } from 'lucide-react';
import { useAppStore, type SettingsCategoryId } from '@/stores/app-store';
import { ProjectEnvSection } from './sections/ProjectEnvSection';
import { ConnectionSection } from './sections/ConnectionSection';
import { AppearanceSection } from './sections/AppearanceSection';
import { RequestSection } from './sections/RequestSection';
import { cn } from '@/lib/utils';
import type { HueConcept } from '@/lib/hue';
import { IconTile } from '@/components/ui/icon-tile';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';

const CATEGORY_IDS: readonly SettingsCategoryId[] = ['env', 'connection', 'appearance', 'request'];

const RAIL_ITEMS: {
  id: SettingsCategoryId;
  label: string;
  icon: React.ReactNode;
  hint: string;
  /**
   * 栏上药丸的语义色相，使激活行的图标色调与分区头用的是同一个概念。
   *
   * `env` 与 `appearance` 不取色相：环境是应用的主要语境（`--primary`），外观是纯装饰，
   * 两者都保持中性，而不是凭空发明一个没有信息收益的颜色。见 `src/lib/hue.ts`。
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
    hint: '超时 / 重定向 / 历史条数',
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
    description: '出站请求的行为，以及发送历史的保留条数',
    icon: <Send className="h-3.5 w-3.5" />,
  },
};

export function SettingsPanel() {
  // 当前分区住在 store，这样顶栏环境切换器的深链（它调 `openSettingsTab('env')`）
  // 在标签首次打开时就落到正确的分区上。
  const activeSection = useAppStore((s) => s.activeSettingsSection);
  const setActiveSection = useAppStore((s) => s.setActiveSettingsSection);

  // 这里不再读别的。环境分区的 store 接线 —— 哪些环境属于当前项目、哪些模块是前置 URL
  // 表的行轴，以及七个环境 / 四个服务动作 —— 住在 `ProjectEnvSection`，因为项目标签挂载的
  // 正是同一份分区。改在这里拼这些 props，就等于让那些规则有第二份副本。
  const mainRef = useRef<HTMLElement>(null);

  // `activeSection` 变化时给当前分区做交叉淡入。外层包装带 `key={activeSection}`，
  // React 因此每次切换都重挂分区，我们再把刚挂上的根节点从「淡出 + 略微下移」补间回
  // 它的自然位置。
  //
  // 纯装饰：分区无论如何都会重挂，故 reduced motion 下它只是直接出现。不需要结束回调
  // —— 补间背后没有状态，不像树的展开高度或响应面板的数字滚动。
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
      {/* 左栏 —— 分区条目的竖排列表。视觉分量对齐项目树，让设置标签读起来像又一个导航
          语境，而不是一个弹窗。 */}
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
                  // 激活 = 中性填充 + 前缘 3px 强调条。描边读作「这是个盒子」，
                  // 而强调条读作「你在这里」。
                  'group flex items-center gap-2 rounded-md py-1.5 pl-2.5 pr-2 text-left text-xs transition-colors',
                  active
                    ? 'left-accent bg-surface-select text-foreground'
                    : 'hover:bg-overlay-hover',
                )}
              >
                <IconTile
                  size="xs"
                  icon={item.icon}
                  className={cn(
                    'transition-colors',
                    !active && 'bg-muted text-faint group-hover:text-muted-foreground',
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

        {/* 钉在底部的小提示卡 —— 说明改动会立刻保存、哪些项要重启才生效。 */}
        <div className="mt-auto rounded-md border border-dashed border-border/70 bg-background/30 px-2.5 py-2 text-[11px] leading-relaxed text-muted-foreground">
          设置改动会立即保存。重启 MCP 进程后部分项生效。
        </div>
      </aside>

      {/* 主内容区。一段细窄的面包屑 + 分区标题给滚动区开头，分区正文滚下去之后用户
          仍有一个稳定的「我在哪」提示。 */}
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
            return (
              <div key={activeSection} data-settings-section>
                <ProjectEnvSection />
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
