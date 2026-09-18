import { Cpu, FileJson, Bot, Server, Activity, Copy, RefreshCw } from 'lucide-react';
import { useEffect, useState } from 'react';
import { useAppStore } from '@/stores/app-store';
import { SettingsCategory } from '../SettingsCategory';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  McpStatusPanel,
  StateSummary,
} from './McpStatusPanel';
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
        command: 'simple-post-mcp-proxy',
        args: ['stdio'],
        env: {
          SIMPLE_POST_DATA_DIR: '%APPDATA%\\\\simple-post',
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
                <span className="text-sm font-medium text-foreground">
                  simple-post-mcp
                </span>
                <Badge variant="muted" className="font-mono">stdio</Badge>
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
            <Badge variant="muted" className="shrink-0 font-mono">JSON-RPC 2.0</Badge>
          </div>
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
