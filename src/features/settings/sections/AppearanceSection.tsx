import { Monitor, Moon, Sun } from 'lucide-react';
import { useAppStore, type Theme } from '@/stores/app-store';
import { SettingsCategory } from '../SettingsCategory';
import { cn } from '@/lib/utils';

function ThemeOption({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: Theme;
}) {
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const active = theme === value;
  return (
    <button
      type="button"
      onClick={() => setTheme(value)}
      aria-pressed={active}
      data-testid={`theme-option-${value}`}
      className={cn(
        'inline-flex items-center gap-2 rounded-md border px-3 py-1.5 text-xs transition-colors',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background',
        active
          ? 'border-primary bg-primary/10 text-foreground'
          : 'border-border bg-card/30 text-muted-foreground hover:bg-accent hover:text-foreground',
      )}
    >
      {icon}
      {label}
    </button>
  );
}

const KEYBOARD_SHORTCUTS: ReadonlyArray<readonly [string, readonly string[]]> = [
  ['发送请求', ['Ctrl', 'Enter']],
  ['保存', ['Ctrl', 'S']],
  ['命令面板', ['Ctrl', 'K']],
  ['切换环境', ['Ctrl', 'E']],
  ['新建接口', ['Ctrl', 'N']],
  ['关闭标签', ['Ctrl', 'W']],
];

export function AppearanceSection() {
  return (
    <>
      <SettingsCategory title="外观" description="主题跟随系统可自动响应 Windows 主题切换">
        {/* The three theme options form a small switch group — kept compact
            so it doesn't sprawl across the full right pane width. */}
        <div className="inline-flex flex-wrap items-center gap-2 rounded-md border border-border bg-card/40 p-1">
          <ThemeOption icon={<Moon className="h-3.5 w-3.5" />} label="暗色" value="dark" />
          <ThemeOption icon={<Sun className="h-3.5 w-3.5" />} label="亮色" value="light" />
          <ThemeOption icon={<Monitor className="h-3.5 w-3.5" />} label="跟随系统" value="system" />
        </div>
      </SettingsCategory>

      <SettingsCategory title="快捷键" hideDivider>
        {/* Two-column key/value list, capped at a comfortable reading
            width so the rows don't stretch to the edge of the pane. */}
        <div className="grid max-w-xl grid-cols-[1fr_auto] gap-x-6 gap-y-1.5 text-xs">
          {KEYBOARD_SHORTCUTS.map(([label, keys]) => (
            <div key={label} className="contents">
              <span className="self-center text-muted-foreground">{label}</span>
              <span className="flex items-center justify-end gap-1">
                {keys.map((k, i) => (
                  <span key={i} className="kbd">{k}</span>
                ))}
              </span>
            </div>
          ))}
        </div>
      </SettingsCategory>
    </>
  );
}
