import { useMemo } from 'react';
import { Hourglass, Repeat, Database, RotateCcw, Send } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { SettingsCategory, SettingRow } from '../SettingsCategory';

// 「恢复默认恢复到哪里」的唯一真相。做成常量，好让「恢复默认」处理器与禁用态判定对
// 「什么算被改动过」保持一致。
const REQUEST_DEFAULTS = {
  timeoutMs: 30_000,
  followRedirects: true,
  maxResponseSizeKb: 5_120,
} as const;

/**
 * 请求行为 —— 作用于每一次外发 HTTP 调用。
 *
 * 三行，各带标签、一句说明默认值*为何*重要的描述，以及右侧的控件。「恢复默认」按钮放在
 * 小节表头，且只有当至少一行已偏离 `REQUEST_DEFAULTS` 时才可用，免得这个便利性一直引诱
 * 用户去「重置」本来就已是默认值的东西。
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

      {/* 同一张卡片内的页脚 —— 一句小提醒，说明这里的取值在当前管线中才真正起作用。
          让用户不至于跑去看文档也知道自己身在何处。 */}
      <div className="flex items-center gap-2 border-t border-border/60 bg-muted/20 px-4 py-2 text-[11px] text-muted-foreground">
        <Badge variant="muted" className="font-mono">
          v0.2
        </Badge>
        <span>以下设置（超时 / 跟随重定向 / 最大响应体积）仅在真实请求模式下生效。</span>
      </div>
    </SettingsCategory>
  );
}
