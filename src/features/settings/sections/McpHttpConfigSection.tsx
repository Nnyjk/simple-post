import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { Server, RefreshCw } from 'lucide-react';
import { SettingsCategory } from '../SettingsCategory';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { IS_TAURI_SHELL, readApiBase } from '@/lib/tauri-commands';
import { mcpEndpointUrl } from '@/lib/mcp-client-config';
import { useAppStore } from '@/stores/app-store';
import { focusRing, cn } from '@/lib/utils';
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
 * Streamable-HTTP 监听配置。这张卡片**只管配置**，从不启停 daemon。运行期控制只归「MCP
 * 进程」那一张卡片，本卡片负责持久化 `mcp_http_config_get` / `mcp_http_config_set` 读写的
 * 配置。
 *
 * 监听无法就地改绑，故保存只落配置，由后端回两条重启判断（守护进程那条、MCP 专用监听器
 * 那条）；页脚照此说明并指向拥有重启的那个对象。
 *
 * 已落盘的那份配置住在 store 里，故「接入配置」那张地址卡片读到的是同一份：卡片保存之后
 * 地址卡片立刻跟着变，不会停在旧端口上。
 */
export function McpHttpConfigSection() {
  const saved = useAppStore((s) => s.mcpHttpConfig);
  const refreshMcpHttpConfig = useAppStore((s) => s.refreshMcpHttpConfig);
  const saveMcpHttpConfig = useAppStore((s) => s.saveMcpHttpConfig);
  const [draft, setDraft] = useState<McpHttpConfigDraft>(() =>
    draftFromConfig(DEFAULT_MCP_HTTP_CONFIG),
  );
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  /** 守护进程要不要重启才吃到新配置（绑定地址、路径、远程开关、白名单）。 */
  const [restartPending, setRestartPending] = useState(false);
  /** MCP 专用监听器要不要由 Web 服务重新开一次（端口、路径、远程开关、白名单）。 */
  const [listenerRestartPending, setListenerRestartPending] = useState(false);

  const hydrate = useCallback(async () => {
    setLoadError(null);
    const ok = await refreshMcpHttpConfig();
    if (!ok) {
      setLoadError('读取 HTTP 服务配置失败');
    }
  }, [refreshMcpHttpConfig]);

  useEffect(() => {
    void hydrate();
  }, [hydrate]);

  // 表单草稿跟着 store 里那份已落盘配置走：读回来时播种一次，保存后服务端回显的规范化
  // 配置也会重新播种一次（那正是「已修改」标志要清掉的时候）。
  useEffect(() => {
    if (saved) setDraft(draftFromConfig(saved));
  }, [saved]);

  const loading = saved === null;

  // 派生而非存储：对本地表单状态做一个不碰 selector 的 `useMemo`，故这里不存在
  // 「Zustand selector 里新建数组」那个坑。
  const dirty = useMemo(() => (saved ? isDraftDirty(draft, saved) : false), [draft, saved]);
  const error = useMemo(() => draftError(draft), [draft]);
  const remoteRedundant = !draft.allowRemote && isLoopbackHost(draft.host);

  /**
   * 保存后要重启什么。
   *
   * 两个对象的重启动作不一样，故要说清是哪一件：
   *
   * - **守护进程** —— 它的监听地址、路径与白名单是 spawn 时从这一行读的，改完要重启守护
   *   进程才生效。
   * - **MCP 专用监听器** —— 它是 **Web 进程启动时**开的监听器，重启守护进程对它没有影响；
   *   Web 形态下（容器）那部分要重启 Web 服务本身。
   *
   * 指错重启动作比不说更糟：用户在「进程」卡片里重启了守护进程，端口还是没变。
   */
  const restartNotes: string[] = [];
  if (restartPending) {
    restartNotes.push(
      IS_TAURI_SHELL
        ? '监听地址、路径与白名单的改动需要重启「MCP 进程」卡片里的守护进程后生效。'
        : '监听地址、路径与白名单的改动需要重启「MCP 进程」卡片里的守护进程后生效。',
    );
  }
  if (listenerRestartPending) {
    restartNotes.push(
      IS_TAURI_SHELL
        ? '端口一栏只对 Web 与容器形态有效：桌面形态的 MCP 由守护进程应答，它的端口由系统分配。'
        : '端口的改动需要重启 Web 服务（容器）本身后生效 —— MCP 专用端口是 Web 进程启动时开的，重启守护进程对它没有影响。',
    );
  }

  /**
   * 对外暴露的 MCP 端点（Web 与容器形态）。取的是**已保存**的配置，故它报的是当前生效的
   * 那个端点，而不是草稿里还没保存的那个。
   */
  const exposedEndpoint = useMemo(() => {
    if (IS_TAURI_SHELL) return null;
    return mcpEndpointUrl({
      shape: 'web',
      origin: typeof window === 'undefined' ? '' : window.location.origin,
      basePath: readApiBase(),
      mcpPort: saved?.port ?? 0,
      mcpPath: saved?.path ?? '/mcp',
    });
  }, [saved]);

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
      const result = await saveMcpHttpConfig(config);
      if (!result) {
        setSaveError('保存失败，请重试');
        return;
      }
      setRestartPending(result.requiresRestart === true);
      setListenerRestartPending(result.requiresListenerRestart === true);
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
          <Field label="端口" hint="0 = 不单独开 MCP 端口">
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
        <p className="text-[11px] text-muted-foreground/80">
          端口写 0 时，MCP 只走主端口上的 /mcp；写一个具体端口时，服务端会另开一个 只服务 MCP
          的监听器（那个端口上没有管理平台，也没有 REST 接口），这个端口 就是对外暴露 MCP
          的端口——主端口上的 /mcp 照旧可用。桌面形态下 MCP 由后台守护 进程应答，它的端口
          始终由系统分配，这一栏只对 Web 与容器形态有意义。
        </p>
        {!IS_TAURI_SHELL && exposedEndpoint && (
          <div className="rounded-md border border-border/60 bg-muted/30 px-2.5 py-2">
            <div className="text-[11px] font-medium text-foreground">对外暴露的 MCP 端点</div>
            <div className="mt-0.5 font-mono text-[11px] text-foreground/90 break-all">
              {exposedEndpoint}
            </div>
            <div className="mt-1 text-[10px] text-muted-foreground/80">
              {saved && saved.port > 0
                ? `专用端口 ${saved.port} 是 Web 服务启动时开的监听器，改端口要重启 Web 服务；部署侧还要把这个端口发布出来。`
                : 'MCP 只走主端口上的 /mcp。'}
            </div>
          </div>
        )}
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
          ) : restartNotes.length > 0 ? (
            <span>{restartNotes.join(' ')}</span>
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
 * 一个白名单文本域。`components/ui/` 里没有共享的 `Textarea` 原语，而为唯一调用方加一个
 * 只是死重量，故这里内联镜像 `Input` 的类名约定。
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
