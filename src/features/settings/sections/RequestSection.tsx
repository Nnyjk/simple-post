import { useAppStore } from '@/stores/app-store';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { SettingsCategory } from '../SettingsCategory';

export function RequestSection() {
  const settings = useAppStore((s) => s.requestSettings);
  const updateRequestSettings = useAppStore((s) => s.updateRequestSettings);

  // No first-level title and no sub-section title — the left rail
  // already labels this as "请求". The form is the page.
  return (
    <SettingsCategory hideDivider>
      <div className="max-w-xl space-y-3">
        <div className="grid grid-cols-[120px_180px] items-center gap-3">
          <label htmlFor="req-timeout" className="text-xs text-muted-foreground">
            超时（ms）
          </label>
          <Input
            id="req-timeout"
            type="number"
            min={1000}
            step={1000}
            value={settings.timeoutMs}
            onChange={(e) => {
              const n = Number(e.target.value);
              if (Number.isFinite(n) && n >= 1000) updateRequestSettings({ timeoutMs: n });
            }}
            className="h-7 font-mono text-xs"
          />
        </div>
        <div className="grid grid-cols-[120px_180px] items-center gap-3">
          <label htmlFor="req-follow" className="text-xs text-muted-foreground">
            跟随重定向
          </label>
          <div className="flex items-center gap-2">
            <Switch
              id="req-follow"
              checked={settings.followRedirects}
              onCheckedChange={(v) => updateRequestSettings({ followRedirects: v })}
            />
            <span className="text-[11px] text-muted-foreground">
              {settings.followRedirects ? 'on · 最多 10 次' : 'off · 手动处理 3xx'}
            </span>
          </div>
        </div>
        <div className="grid grid-cols-[120px_180px] items-center gap-3">
          <label htmlFor="req-maxsize" className="text-xs text-muted-foreground">
            最大响应体积（KB）
          </label>
          <Input
            id="req-maxsize"
            type="number"
            min={1}
            step={64}
            value={settings.maxResponseSizeKb}
            onChange={(e) => {
              const n = Number(e.target.value);
              if (Number.isFinite(n) && n >= 1) updateRequestSettings({ maxResponseSizeKb: n });
            }}
            className="h-7 font-mono text-xs"
          />
        </div>
      </div>
    </SettingsCategory>
  );
}
