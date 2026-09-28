import { Cpu, FileJson, Bot, Server, Activity, Copy, RefreshCw, Power } from 'lucide-react';
import { useEffect, useState } from 'react';
import { useAppStore } from '@/stores/app-store';
import { SettingsCategory } from '../SettingsCategory';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { McpStatusPanel, StateSummary } from './McpStatusPanel';
import { CopyConfigTile } from './CopyConfigTile';
import { AgentPermissionsTable } from './AgentPermissionsTable';
import { DaemonRestartDialog } from './DaemonRestartDialog';
import { COMMANDS } from '@/lib/tauri-commands';
import { invokeCmd } from '@/lib/tauri-commands';

const GENERIC_CONFIG = JSON.stringify(
  {
    mcpServers: {
      'simple-post': {
        command: 'simple-post',
        args: ['mcp'],
        env: {
          SIMPLE_POST_DATA_DIR: '%APPDATA%\\\\simple-post',
        },
        metadata: {
          description: 'Simple Post — 本地 API 工具的 MCP Server',
          version: '0.1.0',
        },
      },
      'simple-post-stdio': {
        // plan commit 4: `simple-post-mcp-proxy` was removed in
        // commit 3; the standalone bridge binary is now
        // `simple-post-mcp`. For one-step install users should
        // prefer the npm wrapper (`npx -y @simple-post/mcp-server`).
        command: 'simple-post-mcp',
        args: ['stdio'],
        env: {
          SIMPLE_POST_DATA_DIR: '%APPDATA%\\\\simple-post',
        },
      },
      'simple-post-npm': {
        command: 'npx',
        args: ['-y', '@simple-post/mcp-server'],
        env: {
          SIMPLE_POST_DATA_DIR: '%APPDATA%\\\\simple-post',
        },
        metadata: {
          description:
            'npm wrapper around the standalone simple-post-mcp bridge — no native install needed',
        },
      },
    },
  },
  null,
  2,
);

export function ConnectionSection() {
  // Selector: raw ref only — see app-store rules.
  const projects = useAppStore((s) => s.projects);

  // Subscribe to the shared MCP daemon snapshot. `useMcpStatus` is
  // the *only* place polling is started — any other consumer (TopBar
  // pill, etc.) just reads `mcpStatus` and gets the same payload.
  const mcpStatus = useAppStore((s) => s.mcpStatus);

  // MCP auto-start preference — drives the "启动配置" Switch below.
  // The slice hydrates this on the first read; the panel renders an
  // indeterminate state until then. Default is `false` at first
  // launch: an MCP server running is explicit user intent.
  const mcpAutoStart = useAppStore((s) => s.mcpAutoStart);
  const setMcpAutoStart = useAppStore((s) => s.setMcpAutoStart);
  const refreshMcpAutoStart = useAppStore((s) => s.refreshMcpAutoStart);
  const [autoStartBusy, setAutoStartBusy] = useState(false);
  useEffect(() => {
    void refreshMcpAutoStart();
  }, [refreshMcpAutoStart]);

  // ----- MCP capability policy -------------------------------
  // Three-tier global capability that filters `tools/list` for AI
  // clients. Stored in sqlite `preferences.mcp.policy`; UI hydrates
  // on mount, persists on change. An absent key resolves to
  // `read_only`, so a fresh install cannot be written to.
  type CapabilityPolicy = 'read_only' | 'safe_write' | 'full_access';
  const [capability, setCapability] = useState<CapabilityPolicy | null>(null);
  const [capabilityBusy, setCapabilityBusy] = useState(false);
  const [capabilityTools, setCapabilityTools] = useState<string[]>([]);
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const policy = await invokeCmd<{ policy: CapabilityPolicy }>(COMMANDS.mcpCapabilityGet);
        if (cancelled) return;
        setCapability(policy.policy);
        const tools = await invokeCmd<string[]>(COMMANDS.mcpCapabilityListTools);
        if (cancelled) return;
        setCapabilityTools(tools);
      } catch (e) {
        console.error('[mcp capability] hydrate failed', e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);
  const handleCapabilityChange = async (next: CapabilityPolicy) => {
    if (next === capability) return;
    setCapabilityBusy(true);
    try {
      await invokeCmd(COMMANDS.mcpCapabilitySet, { policy: next });
      setCapability(next);
      const tools = await invokeCmd<string[]>(COMMANDS.mcpCapabilityListTools);
      setCapabilityTools(tools);
    } catch (e) {
      console.error('[mcp capability] set failed', e);
    } finally {
      setCapabilityBusy(false);
    }
  };
  const handleAutoStartToggle = async (next: boolean) => {
    setAutoStartBusy(true);
    try {
      await setMcpAutoStart(next);
    } catch (e) {
      console.error('[settings] mcp.auto_start toggle failed', e);
    } finally {
      setAutoStartBusy(false);
    }
  };

  // Auto-open the restart dialog when the daemon crashes or goes
  // unhealthy. Dedup by `status` so two consecutive `crashed` polls
  // don't reopen the dialog after the user dismisses it.
  const restartDialogSeen = useAppStore((s) => s.restartDialogSeen);
  const markRestartDialogSeen = useAppStore((s) => s.markRestartDialogSeen);
  const restartMcp = useAppStore((s) => s.restartMcp);
  const [showRestartDialog, setShowRestartDialog] = useState(false);
  useEffect(() => {
    const status = mcpStatus?.status;
    if (!status) return;
    const bad = status === 'crashed' || status === 'unhealthy';
    if (bad && restartDialogSeen !== status) {
      setShowRestartDialog(true);
    }
  }, [mcpStatus?.status, restartDialogSeen]);

  // Proxy binary path — fetched lazily from the daemon on mount.
  //
  // Three states to render:
  //   - `proxyPathAvailable` true   → show the daemon-reported path
  //   - `proxyPathAvailable` false during the loading window → show
  //     a "正在解析…" badge rather than the previous `<you>` literal
  //   - on invoke reject (Worker hasn't shipped the command yet,
  //     very early dev mode) → the path slot is hidden until the
  //     daemon starts serving it. The user can force a retry via the
  //     "重新载入" button.
  const [proxyPath, setProxyPath] = useState<string | null>(null);
  const [proxyPathAvailable, setProxyPathAvailable] = useState(false);
  const [proxyPathLoading, setProxyPathLoading] = useState(true);
  const refreshProxyPath = () => {
    setProxyPathLoading(true);
    invokeCmd<string>(COMMANDS.mcpDaemonProxyPath)
      .then((p) => {
        if (typeof p === 'string' && p.length > 0) {
          setProxyPath(p);
          setProxyPathAvailable(true);
        }
      })
      .catch(() => {
        // Worker hasn't shipped the command (or the daemon isn't
        // ready) — leave the slot hidden until the user retries.
        setProxyPathAvailable(false);
      })
      .finally(() => {
        setProxyPathLoading(false);
      });
  };
  useEffect(() => {
    refreshProxyPath();
  }, []);

  const [copyState, setCopyState] = useState<'idle' | 'success' | 'error'>('idle');
  const handleCopyProxy = async () => {
    if (!proxyPath) return;
    try {
      if (navigator?.clipboard?.writeText) {
        await navigator.clipboard.writeText(proxyPath);
      } else {
        const ta = document.createElement('textarea');
        ta.value = proxyPath;
        ta.style.position = 'fixed';
        ta.style.opacity = '0';
        document.body.appendChild(ta);
        ta.select();
        document.execCommand('copy');
        document.body.removeChild(ta);
      }
      setCopyState('success');
    } catch {
      setCopyState('error');
    }
    window.setTimeout(() => setCopyState('idle'), 2000);
  };

  const tools = mcpStatus?.registeredTools ?? [];

  return (
    <>
      {/* ----- MCP process — hero card ---------------------------- */}
      <SettingsCategory
        title="MCP 进程"
        description="stdio 模式 · 供 Claude Desktop、Cursor 等本地调用"
        icon={<Server className="h-3.5 w-3.5" />}
        meta="进程级状态"
        hideDivider
      >
        <div className="flex flex-col gap-4 p-4 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-start gap-3">
            <div className="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-primary/15 text-primary">
              <Cpu className="h-5 w-5" />
            </div>
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-foreground">simple-post-mcp</span>
                <Badge variant="muted" className="font-mono">
                  stdio
                </Badge>
              </div>
              <div className="mt-1 text-[11px]">
                <StateSummary state={mcpStatus} />
              </div>
            </div>
          </div>
          <McpStatusPanel />
        </div>
        <div className="grid grid-cols-1 gap-3 border-t border-border/60 p-3 sm:grid-cols-2">
          <div className="flex items-start gap-3 px-1">
            <span className="mt-0.5 inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-muted/60 text-muted-foreground">
              <Activity className="h-3.5 w-3.5" />
            </span>
            <div className="min-w-0 flex-1">
              <div className="text-xs font-medium text-foreground">已注册工具</div>
              <div className="mt-0.5 text-[11px] text-muted-foreground">
                stdio JSON-RPC 接口，可被任何支持 MCP 的客户端调用
              </div>
            </div>
            <Badge variant="muted" className="shrink-0 font-mono">
              {tools.length} 个
            </Badge>
          </div>
          <div className="flex items-start gap-3 border-t border-border/60 px-1 pt-3 sm:border-l sm:border-t-0 sm:pl-4 sm:pt-0">
            <span className="mt-0.5 inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-muted/60 text-muted-foreground">
              <FileJson className="h-3.5 w-3.5" />
            </span>
            <div className="min-w-0 flex-1">
              <div className="text-xs font-medium text-foreground">传输协议</div>
              <div className="mt-0.5 text-[11px] text-muted-foreground">
                默认 stdio，无网络监听面 — 进程级安全
              </div>
            </div>
            <Badge variant="muted" className="shrink-0 font-mono">
              JSON-RPC 2.0
            </Badge>
          </div>
        </div>
      </SettingsCategory>

      {/* ----- MCP capability policy --------------------------- */}
      {/* Three-tier global capability that filters `tools/list` for AI
          clients. Persisted to `preferences.mcp.policy`; it is the
          global lever, while the per-project `permissions` table
          still gates individual mutations. */}
      <SettingsCategory
        title="AI 写入权限"
        description="控制 AI 客户端可见的工具集。切换后立刻生效，无需重启 daemon。"
        icon={<Bot className="h-3.5 w-3.5" />}
        meta={
          capability === null
            ? '加载中…'
            : capability === 'read_only'
              ? '只读'
              : capability === 'safe_write'
                ? '读写'
                : '全部'
        }
      >
        <div className="space-y-3 p-3">
          <CapabilityRadioGroup
            value={capability}
            disabled={capabilityBusy || capability === null}
            onChange={handleCapabilityChange}
          />
          {capability !== null && (
            <div className="text-[11px] text-muted-foreground">
              当前可见 {capabilityTools.length} 个工具
              {capabilityTools.length > 0 ? '：' : ''}
              {capabilityTools.slice(0, 8).join('、')}
              {capabilityTools.length > 8 ? '…' : ''}
            </div>
          )}
        </div>
      </SettingsCategory>

      {/* ----- MCP auto-start (前/后台分布 -- 默认 off) ----------- */}
      {/* 渲染面板，默认目标为 false。Tauri 形态 + serve-web 形态都尊重这个键，
          所以一处开关覆盖两种形态。 */}
      <SettingsCategory
        title="启动配置"
        description="决定 simple-post 启动时是否自动拉起 MCP 子进程；关闭后由用户手动开启"
        icon={<Power className="h-3.5 w-3.5" />}
        meta={mcpAutoStart === null ? '加载中…' : mcpAutoStart ? '已开启' : '已关闭'}
        hideDivider
      >
        <div className="flex items-center justify-between gap-3 p-3">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2 text-xs font-medium text-foreground">
              启动时自动拉起 MCP 子进程
            </div>
            <div className="mt-0.5 text-[11px] text-muted-foreground">
              关闭后 MCP 不会随应用启动；可在下方「MCP 进程」卡片里点击「立即启动」手起。
              适用于需要按需启动 daemon 的场景（例如调试、CI、限制后台进程）。
            </div>
          </div>
          <Switch
            checked={mcpAutoStart === true}
            disabled={mcpAutoStart === null || autoStartBusy}
            onCheckedChange={handleAutoStartToggle}
            aria-label="启动时自动拉起 MCP 子进程"
          />
        </div>
      </SettingsCategory>

      {/* ----- Snippet card (live tool list) ---------------------- */}
      <SettingsCategory
        title="可用工具清单"
        description={
          tools.length > 0
            ? `${tools.length} 个工具 · 由 daemon 实时上报`
            : 'daemon 未上报工具清单，重启 MCP 后会刷新'
        }
        icon={<FileJson className="h-3.5 w-3.5" />}
        meta={tools.length > 0 ? `${tools.length} 个工具` : '0 个工具'}
        hideDivider
      >
        <pre className="m-3 max-h-40 overflow-auto rounded-md border border-border/70 bg-muted/40 p-3 font-mono text-[11px] leading-relaxed text-foreground/90">
          {tools.length > 0 ? tools.join('\n') : '(empty — daemon 未就绪)'}
        </pre>
        <div className="m-3 mt-0 flex items-center justify-between gap-3">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2 text-[11px] font-medium text-foreground">
              代理二进制路径
              {!proxyPathAvailable && !proxyPathLoading && (
                <span className="rounded-full bg-muted px-2 py-0.5 text-[10px] font-normal text-muted-foreground">
                  实际路径待打包后显示（开发模式占位）
                </span>
              )}
            </div>
            {proxyPathAvailable && proxyPath ? (
              <div
                className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground"
                title={proxyPath}
              >
                {proxyPath}
              </div>
            ) : (
              <div className="mt-0.5 text-[10px] text-muted-foreground/70">
                {proxyPathLoading ? '解析中…' : '尚未解析到路径'}
              </div>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-1.5">
            {!proxyPathAvailable && !proxyPathLoading && (
              <Button
                type="button"
                variant="ghost"
                size="xs"
                onClick={refreshProxyPath}
                className="gap-1.5"
              >
                <RefreshCw className="h-3 w-3" />
                重新载入
              </Button>
            )}
            <Button
              type="button"
              variant={copyState === 'success' ? 'secondary' : 'outline'}
              size="xs"
              onClick={handleCopyProxy}
              disabled={!proxyPathAvailable}
              className="gap-1.5"
            >
              {copyState === 'success' ? (
                <>已复制</>
              ) : copyState === 'error' ? (
                <>复制失败</>
              ) : (
                <>
                  <Copy className="h-3 w-3" />
                  复制代理路径
                </>
              )}
            </Button>
          </div>
        </div>
      </SettingsCategory>

      {/* ----- Client-side config ---------------------------------- */}
      <SettingsCategory
        title="接入配置"
        description="通用 MCP 配置，贴到任何支持 mcpServers 的客户端即可"
        icon={<FileJson className="h-3.5 w-3.5" />}
        hideDivider
      >
        <div className="p-3">
          <CopyConfigTile
            icon={<FileJson className="h-4 w-4" />}
            name="通用 JSON"
            description="兼容 Claude Desktop / Cursor / Continue 等任意 mcpServers 客户端"
            configJson={GENERIC_CONFIG}
          />
        </div>
      </SettingsCategory>

      {/* ----- Agent permissions ----------------------------------- */}
      <SettingsCategory
        title="AI Agent 权限"
        description="按项目分别授权 agent 可执行的操作集。重启 MCP 后生效"
        icon={<Bot className="h-3.5 w-3.5" />}
        meta={`${projects.length} 个项目`}
        hideDivider
      >
        <div className="p-3">
          <AgentPermissionsTable projects={projects} />
        </div>
      </SettingsCategory>

      <DaemonRestartDialog
        open={showRestartDialog}
        onClose={() => {
          // Record what we just dismissed so the same status doesn't
          // immediately reopen the dialog on the next 3s poll.
          if (mcpStatus?.status) markRestartDialogSeen(mcpStatus.status);
          setShowRestartDialog(false);
        }}
        onRestart={async () => {
          await restartMcp();
          if (mcpStatus?.status) markRestartDialogSeen(mcpStatus.status);
          setShowRestartDialog(false);
        }}
        lastError={mcpStatus?.lastError}
      />
    </>
  );
}

// ----- Capability radio group (commit 1) -------------------------
// Three-way switch wired directly to the Tauri command. Plain radio
// inputs keep this independent of the project's `SettingsCategory` and
// store plumbing — the slice-level integration can be added later
// (commit 1a TODO).
type CapabilityPolicy = 'read_only' | 'safe_write' | 'full_access';
const CAPABILITY_OPTIONS: ReadonlyArray<{
  value: CapabilityPolicy;
  label: string;
  hint: string;
}> = [
  { value: 'read_only', label: '只读', hint: 'AI 仅能 list / get，不能修改任何数据。默认。' },
  { value: 'safe_write', label: '读写', hint: '允许创建 / 修改 / 复制，但不发 http_send、不删。' },
  { value: 'full_access', label: '全部', hint: '包括删除与外发 HTTP 请求。AI 可以做任何事。' },
];
function CapabilityRadioGroup({
  value,
  disabled,
  onChange,
}: {
  value: CapabilityPolicy | null;
  disabled: boolean;
  onChange: (next: CapabilityPolicy) => void;
}) {
  return (
    <div className="flex flex-col gap-2">
      {CAPABILITY_OPTIONS.map((opt) => {
        const checked = value === opt.value;
        const id = `mcp-cap-${opt.value}`;
        return (
          <label
            key={opt.value}
            htmlFor={id}
            className={`flex cursor-pointer items-start gap-2 rounded-md border p-2 transition-colors ${
              checked
                ? 'border-primary bg-primary/5'
                : 'border-border bg-background hover:border-primary/40'
            } ${disabled ? 'cursor-not-allowed opacity-50' : ''}`}
          >
            <input
              id={id}
              type="radio"
              name="mcp-capability"
              value={opt.value}
              checked={checked}
              disabled={disabled}
              onChange={() => onChange(opt.value)}
              className="mt-0.5 h-3.5 w-3.5 accent-[var(--color-primary,#3b82f6)]"
            />
            <div className="min-w-0 flex-1">
              <div className="text-xs font-medium text-foreground">{opt.label}</div>
              <div className="mt-0.5 text-[11px] text-muted-foreground">{opt.hint}</div>
            </div>
          </label>
        );
      })}
    </div>
  );
}
