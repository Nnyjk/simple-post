import { FilePlus, Settings as SettingsIcon, BookOpen, Sparkles } from 'lucide-react';
import { useAppStore, type Theme } from '@/stores/app-store';
import { cn } from '@/lib/utils';

const THEME_ICON: Record<Theme, string> = {
  dark: '🌙',
  light: '☀️',
  system: '🖥️',
};

/**
 * Welcome / empty-state screen.
 *
 * Shown when `activeEndpointId === null` — i.e. no endpoint is currently
 * selected. Renders a centered hero block with three primary actions:
 * create a new project, open settings, or jump to the (forthcoming)
 * documentation.
 *
 * The component is "visibility-self-managed": it returns `null` whenever
 * an endpoint is active, so it can be safely rendered in App.tsx without
 * extra conditional logic at the call-site. (Defence-in-depth — App.tsx
 * also short-circuits the render, but this guard makes the contract
 * explicit.)
 */
export function WelcomeScreen() {
  const activeEndpointId = useAppStore((s) => s.activeEndpointId);
  const theme = useAppStore((s) => s.theme);
  const openSettingsTab = useAppStore((s) => s.openSettingsTab);
  const openNewItem = useAppStore((s) => s.openNewItem);

  if (activeEndpointId !== null) return null;

  const handleDocs = () => {
    // placeholder — full docs are planned for v0.2
    window.alert('文档中心：敬请期待\n\nv0.2 上线后将提供完整使用指南与快捷键手册。');
  };

  return (
    <div
      data-testid="welcome-screen"
      className="flex h-full w-full items-center justify-center bg-background p-6 animate-fade-in"
    >
      <div className="w-full max-w-xl space-y-8 text-center">
        {/* Hero */}
        <div className="space-y-3">
          <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-xl bg-gradient-to-br from-blue-500 to-purple-500 shadow-lg">
            <Sparkles className="h-6 w-6 text-white" />
          </div>
          <h1 className="text-2xl font-semibold tracking-tight">
            简单、强大的 API 调试工作台
          </h1>
          <p className="mx-auto max-w-md text-sm text-muted-foreground">
            组织项目 · 编排接口 · 发送请求 · 内置 MCP 让 AI 直接驱动你的工作区。
          </p>
        </div>

        {/* Three CTAs */}
        <div className="grid gap-3 sm:grid-cols-3">
          <CtaCard
            icon={<FilePlus className="h-5 w-5" />}
            title="新建项目"
            description="从零开始一个新工作区"
            onClick={() => openNewItem({ type: 'project' })}
            primary
          />

          <CtaCard
            icon={<SettingsIcon className="h-5 w-5" />}
            title="打开设置"
            description="环境、主题、请求参数"
            onClick={() => openSettingsTab()}
          />

          <CtaCard
            icon={<BookOpen className="h-5 w-5" />}
            title="查看文档"
            description="使用指南与快捷键"
            onClick={handleDocs}
          />
        </div>

        {/* Footnote with current theme indicator */}
        <div className="flex items-center justify-center gap-1.5 text-[11px] text-muted-foreground/70">
          <span>当前主题：</span>
          <span className="font-mono">
            {theme}
            <span className="ml-1" aria-hidden>
              {THEME_ICON[theme]}
            </span>
          </span>
          <span className="mx-1.5">·</span>
          <span>可在设置中切换</span>
        </div>
      </div>
    </div>
  );
}

interface CtaCardProps {
  icon: React.ReactNode;
  title: string;
  description: string;
  onClick: () => void;
  primary?: boolean;
  children?: React.ReactNode;
}

function CtaCard({ icon, title, description, onClick, primary, children }: CtaCardProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'group flex flex-col items-start gap-1 rounded-lg border p-4 text-left transition-colors',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background',
        primary
          ? 'border-primary/50 bg-primary/5 hover:border-primary hover:bg-primary/10'
          : 'border-border bg-card/40 hover:border-primary/40 hover:bg-accent/40',
      )}
    >
      <div
        className={cn(
          'mb-1 flex h-8 w-8 items-center justify-center rounded-md transition-colors',
          primary
            ? 'bg-primary/15 text-primary'
            : 'bg-muted text-muted-foreground group-hover:text-foreground',
        )}
      >
        {icon}
      </div>
      <div className="text-sm font-semibold">{title}</div>
      <div className="text-[11px] text-muted-foreground">{description}</div>
      {children}
    </button>
  );
}
