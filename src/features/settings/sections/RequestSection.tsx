import { useMemo } from 'react';
import { Hourglass, Repeat, Database, RotateCcw, Send } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { SettingsCategory, SettingRow } from '../SettingsCategory';

// Single source of truth for "what does this reset to". Exposed as a
// constant so the "恢复默认" handler and the disabled-state check both
// agree on what counts as a customisation.
const REQUEST_DEFAULTS = {
  timeoutMs: 30_000,
  followRedirects: true,
  maxResponseSizeKb: 5_120,
} as const;

/**
 * Request behaviour — applies to every outbound HTTP call.
 *
 * Three rows, each with a label, a description that tells the user *why*
 * the default matters, and a control on the right. A "恢复默认" button
 * sits in the section header; it's only enabled when at least one row
 * has drifted from `REQUEST_DEFAULTS`, so the affordance doesn't keep
 * tempting the user to "reset" things that are already reset.
 */
export function RequestSection() {
  const settings = useAppStore((s) => s.requestSettings);
  const updateRequestSettings = useAppStore((s) => s.updateRequestSettings);

  const isDirty = useMemo(
    () =>
      settings.timeoutMs !== REQUEST_DEFAULTS.timeoutMs ||
      settings.followRedirects !== REQUEST_DEFAULTS.followRedirects ||
      settings.maxResponseSizeKb !== REQUEST_DEFAULTS.maxResponseSizeKb,
    [settings],
  );

  const resetToDefaults = () => updateRequestSettings({ ...REQUEST_DEFAULTS });

  return (
    <SettingsCategory
      title="请求行为"
      description="应用到所有 HTTP 发送（v0.2 由 Rust 端消费）"
      icon={<Send className="h-3.5 w-3.5" />}
      meta={`${Object.keys(REQUEST_DEFAULTS).length} 项`}
      actions={
        <Button
          variant="ghost"
          size="xs"
          onClick={resetToDefaults}
          disabled={!isDirty}
          className="gap-1 text-muted-foreground"
        >
          <RotateCcw className="h-3 w-3" />
          恢复默认
        </Button>
      }
      hideDivider
    >
      <SettingRow
        icon={<Hourglass className="h-3.5 w-3.5" />}
        label="请求超时"
        description="单次请求最长等待时间。超过即中断并返回超时错误"
        hint={`默认 ${REQUEST_DEFAULTS.timeoutMs.toLocaleString()} ms`}
        flush
        control={
          <Input
            type="number"
            min={1000}
            step={1000}
            value={settings.timeoutMs}
            onChange={(e) => {
              const n = Number(e.target.value);
              if (Number.isFinite(n) && n >= 1000) {
                updateRequestSettings({ timeoutMs: n });
              }
            }}
            className="h-8 w-32 font-mono text-xs"
          />
        }
      />

      <SettingRow
        icon={<Repeat className="h-3.5 w-3.5" />}
        label="跟随重定向"
        description="自动跟随 3xx，最多 10 次"
        hint={settings.followRedirects ? '开启 · 自动处理 3xx' : '关闭 · 手动处理 3xx'}
        control={
          <Switch
            checked={settings.followRedirects}
            onCheckedChange={(v) => updateRequestSettings({ followRedirects: v })}
          />
        }
      />

      <SettingRow
        icon={<Database className="h-3.5 w-3.5" />}
        label="最大响应体积"
        description="超过此体积的响应会被截断，避免本地内存被异常响应撑爆"
        hint={`默认 ${REQUEST_DEFAULTS.maxResponseSizeKb.toLocaleString()} KB`}
        control={
          <Input
            type="number"
            min={1}
            step={64}
            value={settings.maxResponseSizeKb}
            onChange={(e) => {
              const n = Number(e.target.value);
              if (Number.isFinite(n) && n >= 1) {
                updateRequestSettings({ maxResponseSizeKb: n });
              }
            }}
            className="h-8 w-32 font-mono text-xs"
          />
        }
      />

      {/* Footer inside the same card — a tiny reminder that the values
          shown here only matter in the (current) mock pipeline. Keeps the
          user oriented without forcing them to scroll the doc. */}
      <div className="flex items-center gap-2 border-t border-border/60 bg-muted/20 px-4 py-2 text-[11px] text-muted-foreground">
        <Badge variant="muted" className="font-mono">
          v0.2
        </Badge>
        <span>以下设置（超时 / 跟随重定向 / 最大响应体积）仅在真实请求模式下生效。</span>
      </div>
    </SettingsCategory>
  );
}
