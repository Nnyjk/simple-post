import { Cpu, FileJson, Bot, Server, Activity } from 'lucide-react';
import { useState } from 'react';
import { useAppStore } from '@/stores/app-store';
import { SettingsCategory } from '../SettingsCategory';
import { Badge } from '@/components/ui/badge';
import {
  McpStatusPanel,
  StateSummary,
  type McpUiState,
} from './McpStatusPanel';
import { CopyConfigTile } from './CopyConfigTile';
import { AgentPermissionsTable } from './AgentPermissionsTable';

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
        command: 'C:\\\\Users\\\\<you>\\\\AppData\\\\Local\\\\simple-post\\\\simple-post.exe',
        args: ['mcp'],
        env: {
          SIMPLE_POST_DATA_DIR: 'C:\\\\Users\\\\<you>\\\\AppData\\\\Roaming\\\\simple-post',
        },
      },
    },
  },
  null,
  2,
);

const REGISTERED_TOOLS = [
  'api_list_projects',
  'api_list_endpoints',
  'api_get_endpoint',
  'api_run_request',
];

export function ConnectionSection() {
  // Selector: raw ref only — see app-store rules.
  const projects = useAppStore((s) => s.projects);

  // The MCP hero card and the restart button share state via a single
  // useState. McpStatusPanel pushes its internal status up via
  // onStateChange so both halves render from one source of truth.
  const [mcpState, setMcpState] = useState<McpUiState>({
    status: 'running',
    pid: 12345,
    requestsHandled: 1247,
  });

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
                <StateSummary state={mcpState} />
              </div>
            </div>
          </div>
          <McpStatusPanel onStateChange={setMcpState} />
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
              {REGISTERED_TOOLS.length} 个
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

      {/* ----- Snippet card (pre-formatted tool list) -------------- */}
      <SettingsCategory
        title="可用工具清单"
        description="展开后复制到客户端配置即可生效"
        icon={<FileJson className="h-3.5 w-3.5" />}
        meta={`${REGISTERED_TOOLS.length} 个工具`}
        hideDivider
      >
        <pre className="m-3 max-h-40 overflow-auto rounded-md border border-border/70 bg-muted/40 p-3 font-mono text-[11px] leading-relaxed text-foreground/90">
{REGISTERED_TOOLS.join('\n')}
        </pre>
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
    </>
  );
}