import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { Server, RefreshCw } from 'lucide-react';
import { SettingsCategory } from '../SettingsCategory';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { focusRing, cn } from '@/lib/utils';
import type { McpHttpConfig } from '@/types/domain';
import {
  DEFAULT_MCP_HTTP_CONFIG,
  configFromDraft,
  draftError,
  draftFromConfig,
  isDraftDirty,
  isLoopbackHost,
  type McpHttpConfigDraft,
} from './mcp-http-config-form';

/**
 * Streamable-HTTP listener configuration.
 *
 * This card is **configuration only** — it never starts or stops the
 * daemon. It used to carry its own 启用 switch right above 启动 / 停止 /
 * 重启 in the "MCP 进程" card, and the two drove exactly the same
 * state, so the reader had to work out which one was authoritative.
 * Runtime control now belongs to one card; this one persists what gets
 * read through `mcp_http_config_get` / `mcp_http_config_set`.
 *
 * The listener cannot rebind in place, so saving persists the config and
 * the backend answers with `requiresRestart`; the footer says so and
 * points at the card that owns the restart rather than implying the
 * running service already moved.
 */
export function McpHttpConfigSection() {
  const [saved, setSaved] = useState<McpHttpConfig | null>(null);
  const [draft, setDraft] = useState<McpHttpConfigDraft>(() =>
    draftFromConfig(DEFAULT_MCP_HTTP_CONFIG),
  );
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [restartPending, setRestartPending] = useState(false);

  const hydrate = useCallback(() => {
    setLoading(true);
    setLoadError(null);
    invokeCmd<McpHttpConfig>(COMMANDS.mcpHttpConfigGet)
      .then((config) => {
        setSaved(config);
        setDraft(draftFromConfig(config));
      })
      .catch((e: unknown) => {
        // The command may be absent in an older backend. Keep the form on
        // defaults and say so rather than rendering a blank card.
        console.error('[mcp http config] hydrate failed', e);
        setLoadError('读取 HTTP 服务配置失败');
        setSaved(DEFAULT_MCP_HTTP_CONFIG);
        setDraft(draftFromConfig(DEFAULT_MCP_HTTP_CONFIG));
      })
      .finally(() => {
        setLoading(false);
      });
  }, []);

  useEffect(() => {
    hydrate();
  }, [hydrate]);

  // Derived, not stored: a selector-free `useMemo` over local form state,
  // so there is no new-array-in-a-Zustand-selector hazard here.
  const dirty = useMemo(() => (saved ? isDraftDirty(draft, saved) : false), [draft, saved]);
  const error = useMemo(() => draftError(draft), [draft]);
  const remoteRedundant = !draft.allowRemote && isLoopbackHost(draft.host);

  const patch = (next: Partial<McpHttpConfigDraft>) =>
    setDraft((prev) => {
      const merged = { ...prev, ...next };
      setSaveError(null);
      return merged;
    });

  const handleSave = async () => {
    const config = configFromDraft(draft);
    if (!config || error) return;
    setSaving(true);
    setSaveError(null);
    try {
      const result = await invokeCmd<{ config: McpHttpConfig; requiresRestart: boolean }>(
        COMMANDS.mcpHttpConfigSet,
        { config },
      );
      // Trust the backend's echo — it normalises (trim / default path),
      // so re-seeding from it keeps the dirty flag honest.
      const next = result.config ?? config;
      setSaved(next);
      setDraft(draftFromConfig(next));
      setRestartPending(result.requiresRestart === true);
    } catch (e) {
      console.error('[mcp http config] save failed', e);
      setSaveError('保存失败，请重试');
    } finally {
      setSaving(false);
    }
  };

  return (
    <SettingsCategory
      title="HTTP 服务配置"
      description="监听地址、端口、MCP 路径与访问白名单"
      icon={<Server className="h-3.5 w-3.5" />}
    >
      {/* ----- listener ---------------------------------------- */}
      <div className="space-y-3 p-3">
        <div className="flex items-center justify-between gap-3">
          <div className="text-[11px] font-medium text-muted-foreground">监听配置</div>
          {loadError && (
            <Button
              type="button"
              variant="ghost"
              size="xs"
              onClick={hydrate}
              className="gap-1.5 text-destructive"
            >
              <RefreshCw className="h-3 w-3" />
              重新载入
            </Button>
          )}
        </div>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-[2fr_1fr_2fr]">
          <Field label="监听地址" hint="本机回环">
            <Input
              value={draft.host}
              onChange={(e) => patch({ host: e.target.value })}
              placeholder="127.0.0.1"
              aria-label="监听地址"
              spellCheck={false}
              disabled={loading}
            />
          </Field>
          <Field label="端口" hint="0 = 系统分配">
            <Input
              type="number"
              min={0}
              max={65535}
              value={draft.port}
              onChange={(e) => patch({ port: e.target.value })}
              placeholder="0"
              aria-label="端口"
              disabled={loading}
            />
          </Field>
          <Field label="MCP 路径" hint="必须以 / 开头">
            <Input
              value={draft.path}
              onChange={(e) => patch({ path: e.target.value })}
              placeholder="/mcp"
              aria-label="MCP 路径"
              spellCheck={false}
              disabled={loading}
            />
          </Field>
        </div>
      </div>

      {/* ----- remote access ------------------------------------ */}
      <div className="space-y-3 border-t border-border/60 p-3">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0 flex-1">
            <div className="text-xs font-medium text-foreground">允许远程访问</div>
            <div className="mt-0.5 text-[11px] text-muted-foreground">
              仅当监听地址为非 127.0.0.1 或 ::1 时需要开启。保持监听地址为回环地址时，
              请求只可能来自本机，开关与白名单都不参与判断。
            </div>
          </div>
          <Switch
            checked={draft.allowRemote}
            onCheckedChange={(v) => patch({ allowRemote: v })}
            aria-label="允许远程访问"
          />
        </div>

        {remoteRedundant && (
          <p className="text-[11px] text-muted-foreground/80">
            监听地址是回环地址，远程访问开关与白名单目前都不参与判断。
          </p>
        )}

        {draft.allowRemote && (
          <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
            <Field label="Host 白名单" hint="一行一个，也支持逗号分隔；填 * 表示不校验">
              <ListTextarea
                value={draft.allowedHosts}
                onChange={(v) => patch({ allowedHosts: v })}
                ariaLabel="Host 白名单"
                placeholder={'simple-post:4173\n127.0.0.1:4173'}
                disabled={loading}
              />
            </Field>
            <Field label="Origin 白名单" hint="一行一个，也支持逗号分隔；必须带协议">
              <ListTextarea
                value={draft.allowedOrigins}
                onChange={(v) => patch({ allowedOrigins: v })}
                ariaLabel="Origin 白名单"
                placeholder={'https://claude.ai\nhttp://localhost:5173'}
                disabled={loading}
              />
            </Field>
          </div>
        )}
      </div>

      {/* ----- save ------------------------------------------------ */}
      <div className="flex items-center justify-between gap-3 border-t border-border/60 p-3">
        <div className="min-w-0 flex-1 text-[11px] text-muted-foreground">
          {saveError ? (
            <span className="text-destructive">{saveError}</span>
          ) : error ? (
            <span className="text-destructive">{error}</span>
          ) : dirty ? (
            <span>有未保存的修改。保存只写入配置，运行中的服务不会立即改变。</span>
          ) : restartPending ? (
            <span>配置已保存，需要在「MCP 进程」卡片里重启后生效。</span>
          ) : (
            <span>与已保存的配置一致。</span>
          )}
        </div>
        <Button
          type="button"
          size="sm"
          onClick={handleSave}
          disabled={loading || saving || !dirty || error !== null}
          className="shrink-0"
        >
          {saving ? '保存中…' : '保存并应用'}
        </Button>
      </div>
    </SettingsCategory>
  );
}

/** Label-above-control field, matching the settings rows elsewhere. */
function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="block min-w-0">
      <span className="block text-[11px] font-medium text-foreground">{label}</span>
      {hint && <span className="block text-[10px] text-muted-foreground/80">{hint}</span>}
      <span className="mt-1 block">{children}</span>
    </label>
  );
}

/**
 * One whitelist textarea.
 *
 * There is no shared `Textarea` primitive in `components/ui/` and adding
 * one would be dead weight for a single caller (the `dead-components`
 * sentinel aside, it is a wider API surface than the feature needs), so
 * this mirrors `Input`'s class contract inline.
 */
function ListTextarea({
  value,
  onChange,
  ariaLabel,
  placeholder,
  disabled,
}: {
  value: string;
  onChange: (next: string) => void;
  ariaLabel: string;
  placeholder: string;
  disabled?: boolean;
}) {
  return (
    <textarea
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      aria-label={ariaLabel}
      spellCheck={false}
      disabled={disabled}
      rows={3}
      className={cn(
        'flex w-full resize-y rounded-md border border-input bg-background px-2.5 py-1.5 font-mono text-xs',
        'placeholder:font-sans placeholder:text-muted-foreground/60',
        focusRing,
        'disabled:cursor-not-allowed disabled:opacity-50',
      )}
    />
  );
}
