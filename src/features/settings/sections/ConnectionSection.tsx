import { FileJson, Cpu } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import { SettingsCategory } from '../SettingsCategory';
import { McpStatusPanel } from './McpStatusPanel';
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

  return (
    <>
      <SettingsCategory title="MCP 进程">
        <McpStatusPanel />
        <div className="rounded-md border border-border bg-popover p-3">
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <Cpu className="h-3.5 w-3.5" />
            <span>已注册工具：{REGISTERED_TOOLS.length} 个</span>
          </div>
          <pre className="mt-2 overflow-x-auto rounded bg-muted/50 p-2 font-mono text-[11px] leading-relaxed text-foreground/90">
{REGISTERED_TOOLS.join('\n')}
          </pre>
        </div>
      </SettingsCategory>

      <SettingsCategory
        title="接入配置"
        description="通用 MCP 配置，贴到任何支持 mcpServers 的客户端即可"
      >
        <div className="grid grid-cols-1 gap-3 max-w-2xl">
          <CopyConfigTile
            icon={<FileJson className="h-4 w-4" />}
            name="通用 JSON"
            description="兼容任何支持 mcpServers 的客户端"
            configJson={GENERIC_CONFIG}
          />
        </div>
      </SettingsCategory>

      <SettingsCategory
        title="AI Agent 权限"
        description="按项目分别授权 agent 可执行的操作集"
        hideDivider
      >
        <AgentPermissionsTable projects={projects} />
      </SettingsCategory>
    </>
  );
}
