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
 * Streamable-HTTP 监听配置。
 *
 * 这张卡片**只管配置**，从不启停 daemon。运行期控制只归「MCP 进程」那一张卡片，
 * 本卡片负责持久化 `mcp_http_config_get` / `mcp_http_config_set` 读写的配置。
 *
 * 监听无法就地改绑，故保存只落配置，由后端回 `requiresRestart`；页脚照此说明并指向
 * 拥有重启的那张卡片，不会暗示运行中的服务已经换地址。
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
        // 命令在更旧的后端上可能不存在。让表单留在默认值上并说明，而不是渲染一张空白卡。
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

  // 派生而非存储：对本地表单状态做一个不碰 selector 的 `useMemo`，故这里不存在
  // 「Zustand selector 里新建数组」那个坑。
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
      // 采信后端的回显 —— 它做过规范化（trim / 默认路径），据它重新播种才能让「已修改」
      // 标志保持诚实。
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
      {/* ----- 监听 ------------------------------------------- */}
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

      {/* ----- 远程访问 --------------------------------------- */}
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

      {/* ----- 保存 --------------------------------------------- */}
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

/** 标签在控件上方的字段，与设置面板别处的行一致。 */
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
 * 一个白名单文本域。
 *
 * `components/ui/` 里没有共享的 `Textarea` 原语，而为唯一调用方加一个只是死重量
 * （不算 `dead-components` 那条哨兵，它的 API 面比这个特性需要还宽），故这里内联镜像
 * `Input` 的类名约定。
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
