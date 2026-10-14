import { useMemo, useState } from 'react';
import { Monitor, Moon, Sun, Keyboard, Search, Palette, Check } from 'lucide-react';
import { useAppStore, type Theme } from '@/stores/app-store';
import { Input } from '@/components/ui/input';
import { EmptyHint } from '@/components/ui/empty-hint';
import { SettingsCategory } from '../SettingsCategory';
import { cn, focusRing } from '@/lib/utils';

interface ThemeOption {
  icon: React.ReactNode;
  label: string;
  value: Theme;
  // 说明各模式之间*实际*的差别，不是营销话术。
  hint: string;
  // 色板预览用的四个颜色。读 globals.css 里的 `--swatch-*` token，而不是各自带一份
  // hex：它得同时渲染两个主题，故用不了会随当前主题解算的 `--background` / `--card`；
  // 改用字面量则会在暗色底色从 #0A0A0A 移走之后继续显示旧配色。
  preview: { bg: string; panel: string; text: string; accent: string };
}

const swatch = (theme: 'light' | 'dark') => ({
  bg: `hsl(var(--swatch-${theme}-bg))`,
  panel: `hsl(var(--swatch-${theme}-panel))`,
  text: `hsl(var(--swatch-${theme}-text))`,
  accent: `hsl(var(--swatch-${theme}-accent))`,
});

const THEME_OPTIONS: ThemeOption[] = [
  {
    icon: <Moon className="h-4 w-4" />,
    label: '暗色',
    value: 'dark',
    hint: '默认 · 长时间使用更舒服',
    preview: swatch('dark'),
  },
  {
    icon: <Sun className="h-4 w-4" />,
    label: '亮色',
    value: 'light',
    hint: '白天或强光环境',
    preview: swatch('light'),
  },
  {
    icon: <Monitor className="h-4 w-4" />,
    label: '跟随系统',
    value: 'system',
    hint: 'Windows 主题切换时自动跟随',
    // 各占一半，让这一项读起来像「两者」而不是其中之一。渐变是 CSS 字符串，
    // 故不能写成裸的 `hsl(var(…))`。
    preview: {
      bg: 'linear-gradient(135deg, hsl(var(--swatch-dark-bg)) 50%, hsl(var(--swatch-light-bg)) 50%)',
      panel: 'hsl(var(--muted-foreground))',
      text: 'hsl(var(--swatch-dark-text))',
      accent: 'hsl(var(--swatch-dark-accent))',
    },
  },
];

const KEYBOARD_SHORTCUTS: ReadonlyArray<readonly [string, readonly string[]]> = [
  ['发送请求', ['Ctrl', 'Enter']],
  ['保存', ['Ctrl', 'S']],
  ['命令面板', ['Ctrl', 'K']],
  ['切换环境', ['Ctrl', 'E']],
  ['新建接口', ['Ctrl', 'N']],
  ['关闭标签', ['Ctrl', 'W']],
];

/**
 * 外观偏好 —— 主题 + 快捷键。
 *
 * 主题渲染成三张卡片网格，让用户预览每项*真实*的样子（面板 + 文字 + 强调色），而不是
 * 从一个词的标签去猜。快捷键配一个小过滤框，凭肌肉记忆查的组合不必滚动。
 */
export function AppearanceSection() {
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const [shortcutQuery, setShortcutQuery] = useState('');

  const filteredShortcuts = useMemo(() => {
    const q = shortcutQuery.trim().toLowerCase();
    if (!q) return KEYBOARD_SHORTCUTS;
    return KEYBOARD_SHORTCUTS.filter(
      ([label, keys]) =>
        label.toLowerCase().includes(q) || keys.join(' ').toLowerCase().includes(q),
    );
  }, [shortcutQuery]);

  return (
    <>
      <SettingsCategory
        title="主题"
        description="主题跟随系统可自动响应 Windows 主题切换"
        icon={<Palette className="h-3.5 w-3.5" />}
        meta={`当前 ${THEME_OPTIONS.find((o) => o.value === theme)?.label ?? '—'}`}
        hideDivider
      >
        <div className="grid grid-cols-1 gap-2 p-3 sm:grid-cols-3">
          {THEME_OPTIONS.map((opt) => {
            const active = theme === opt.value;
            return (
              <button
                key={opt.value}
                type="button"
                onClick={() => setTheme(opt.value)}
                aria-pressed={active}
                data-testid={`theme-option-${opt.value}`}
                className={cn(
                  'group flex flex-col gap-2 rounded-md border bg-background/30 p-2.5 text-left transition-all',
                  focusRing,
                  active
                    ? 'border-primary/60 bg-primary/5 shadow-[0_0_0_3px_hsl(var(--primary)/0.10)]'
                    : 'border-border/70 hover:border-border hover:bg-background/60',
                )}
              >
                {/* 迷你预览 —— 用该项的配色画一块小面板，让用户看见自己将要选的东西。 */}
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <span
                      className={cn(
                        'inline-flex h-6 w-6 items-center justify-center rounded-md',
                        active
                          ? 'bg-primary/15 text-primary'
                          : 'bg-muted/60 text-muted-foreground group-hover:text-foreground',
                      )}
                    >
                      {opt.icon}
                    </span>
                    <span className="text-xs font-medium text-foreground">{opt.label}</span>
                  </div>
                  {active && (
                    <span className="inline-flex h-4 w-4 items-center justify-center rounded-full bg-primary text-primary-foreground">
                      <Check className="h-3 w-3" />
                    </span>
                  )}
                </div>
                <div
                  className="relative h-12 overflow-hidden rounded-md border border-border/40"
                  style={{ background: opt.preview.bg }}
                >
                  <div
                    className="absolute left-2 top-2 h-3 w-8 rounded-sm"
                    style={{ background: opt.preview.panel }}
                  />
                  <div
                    className="absolute left-2 bottom-2 h-2 w-12 rounded-sm"
                    style={{ background: opt.preview.text, opacity: 0.85 }}
                  />
                  <div
                    className="absolute right-2 bottom-2 h-2 w-6 rounded-sm"
                    style={{ background: opt.preview.accent }}
                  />
                </div>
                <p className="text-[11px] text-muted-foreground">{opt.hint}</p>
              </button>
            );
          })}
        </div>
      </SettingsCategory>

      <SettingsCategory
        title="快捷键"
        description="全局生效，无输入框焦点时也可使用"
        icon={<Keyboard className="h-3.5 w-3.5" />}
        meta={`${filteredShortcuts.length} / ${KEYBOARD_SHORTCUTS.length} 项`}
        actions={
          <div className="relative">
            <Search className="pointer-events-none absolute left-2 top-1/2 h-3 w-3 -translate-y-1/2 text-muted-foreground/60" />
            <Input
              type="search"
              placeholder="过滤快捷键…"
              value={shortcutQuery}
              onChange={(e) => setShortcutQuery(e.target.value)}
              className="h-7 w-44 pl-7 text-xs"
            />
          </div>
        }
        hideDivider
      >
        {filteredShortcuts.length === 0 ? (
          <EmptyHint className="border-transparent px-4 py-6 text-[11px]">
            没有匹配的快捷键
          </EmptyHint>
        ) : (
          <div className="divide-y divide-border/60">
            {filteredShortcuts.map(([label, keys]) => (
              <div key={label} className="grid grid-cols-[1fr_auto] items-center gap-4 px-4 py-2.5">
                <span className="text-xs text-foreground">{label}</span>
                <span className="flex items-center gap-1">
                  {keys.map((k, i) => (
                    <span key={i} className="kbd">
                      {k}
                    </span>
                  ))}
                </span>
              </div>
            ))}
          </div>
        )}
      </SettingsCategory>
    </>
  );
}
