import { Cpu, FileJson, Bot, Server, Activity } from 'lucide-react';
import { useEffect, useState, type ReactNode } from 'react';
import { useAppStore } from '@/stores/app-store';
import { SettingsCategory } from '../SettingsCategory';
import { Badge } from '@/components/ui/badge';
import { IconTile } from '@/components/ui/icon-tile';
import { Switch } from '@/components/ui/switch';
import { McpStatusPanel, StateSummary, type McpRuntimeStatus } from './McpStatusPanel';
import { McpLogPanel } from './McpLogPanel';
import { CopyConfigTile } from './CopyConfigTile';
import { AgentPermissionsTable } from './AgentPermissionsTable';
import { DaemonRestartDialog } from './DaemonRestartDialog';
import { McpHttpConfigSection } from './McpHttpConfigSection';
import { COMMANDS } from '@/lib/tauri-commands';
import { invokeCmd } from '@/lib/tauri-commands';
import { MCP_CLIENT_PRESETS, buildClientConfig, unavailableReason } from '@/lib/mcp-client-config';
import { daemonTone } from '@/lib/mcp-status-tone';
import { groupTools } from '@/lib/mcp-tool-groups';
import { cn, focusRing } from '@/lib/utils';

export function ConnectionSection() {
  // selector：只取原始引用 —— 见 app-store 规则。
  const projects = useAppStore((s) => s.projects);

  // 共享的 MCP 守护进程快照。`useMcpStatus` 是**唯一**发起轮询的地方 —— 其他消费方
  // （顶栏药丸等）只读 `mcpStatus`，拿到同一份数据。
  const mcpStatus = useAppStore((s) => s.mcpStatus);
  // 主图块报的是守护进程的生命周期，取色随状态走而非用固定的 `connection` 色相
  // —— 否则正在服务的 daemon 会在绿色 `running` 徽标旁画成红色。
  // 见 `src/lib/mcp-status-tone.ts`。
  const daemonStatus: McpRuntimeStatus = mcpStatus?.status ?? 'stopped';

  // MCP 自动启动偏好 —— 驱动下方的「启动配置」开关。slice 首次读取时才 hydrate，
  // 在此之前面板渲染不定态。首次启动默认 `false`：让 MCP 服务跑起来须是用户的明确意图。
  const mcpAutoStart = useAppStore((s) => s.mcpAutoStart);
  const setMcpAutoStart = useAppStore((s) => s.setMcpAutoStart);
  const refreshMcpAutoStart = useAppStore((s) => s.refreshMcpAutoStart);
  const [autoStartBusy, setAutoStartBusy] = useState(false);
  useEffect(() => {
    void refreshMcpAutoStart();
  }, [refreshMcpAutoStart]);

  // ----- MCP capability policy -------------------------------
  // 三档全局能力，过滤 AI 客户端看到的 `tools/list`。存于 sqlite `preferences.mcp.policy`，
  // UI 挂载时 hydrate、变更即持久化。键缺失解析为 `read_only`，故全新安装不可被写。
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

  // daemon 崩溃或不健康时自动弹重启对话框。按 `status` 去重，免得用户关闭后连续两次
  // `crashed` 轮询又把它弹回来。
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

  // ----- Generated client configs -----
  // 由 daemon **实时**事实驱动而非手写常量，故片段里的端口永远是 daemon 实际所在端口。
  const facts = { port: mcpStatus?.port ?? null };

  const tools = mcpStatus?.registeredTools ?? [];
  // 工具表的分类。每次渲染重算而不 memo：输入是 44 元素数组、`groupTools` 单趟完成，
  // `useMemo` 的成本高于收益。
  const toolGroups = groupTools(tools);

  return (
    <>
      {/* =================================================================
          五张卡片按读者的实际需要排序：拉起 daemon → 给它一个地址 → 把配置交给客户端
          → 决定该客户端能做什么 → 看最终的工具集。

          不设小节标题：重复自己卡片标题的标题只是多一行字、不带新信息，故结构由卡片
          自身承担，分隔交给 `SettingsCategory` 的下边框。
          ================================================================= */}

      {/* ----- 守护进程，唯一能启动它的卡片 --------- */}
      <SettingsCategory
        title="MCP 进程"
        description="MCP 守护进程 · 以 Streamable HTTP 暴露给 AI 客户端"
        icon={<Server className="h-3.5 w-3.5" />}
        meta="进程级状态"
      >
        <div className="flex flex-col gap-4 p-4 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-start gap-3">
            <IconTile
              // 取的是状态而非身份 —— 见 `daemonStatus` 处的说明。
              tone={daemonTone(daemonStatus)}
              size="xl"
              className="rounded-lg"
              icon={<Cpu className="h-5 w-5" />}
            />
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-foreground">simple-post-mcp</span>
                <Badge variant="muted" className="font-mono">
                  HTTP
                </Badge>
              </div>
              <div className="mt-1 text-[11px]">
                <StateSummary state={mcpStatus} />
              </div>
            </div>
          </div>
          <McpStatusPanel />
        </div>

        {/* 自动启动只是关于同一个 daemon 的一个开关（上面的按钮已在控制它），故直接置于
            其下，而非隔着卡片与它们竞争。 */}
        <div className="flex items-center justify-between gap-3 border-t border-border/60 p-3">
          <div className="min-w-0 flex-1">
            <div className="text-xs font-medium text-foreground">随应用启动</div>
            <div className="mt-0.5 text-[11px] text-muted-foreground">
              关闭后 MCP 不会随应用启动，可在「MCP 进程」卡片里点「启动」手动拉起。
              关掉本开关不会掐断正在服务的 daemon —— 上面的「停止 / 启动」决定当前这次是否运行，
              这里只决定下次打开应用时。
            </div>
          </div>
          <Switch
            checked={mcpAutoStart === true}
            disabled={mcpAutoStart === null || autoStartBusy}
            onCheckedChange={handleAutoStartToggle}
            aria-label="随应用启动 MCP 进程"
          />
        </div>

        {/* 日志区：默认折叠，且折叠时不拉日志——日志可能有几百行，每 3s 拉一次
            是白花的带宽。 */}
        <McpLogPanel />

        <div className="grid grid-cols-1 gap-3 border-t border-border/60 p-3 sm:grid-cols-2">
          <div className="flex items-start gap-3 px-1">
            <IconTile className="mt-0.5" icon={<Activity className="h-3.5 w-3.5" />} />
            <div className="min-w-0 flex-1">
              <div className="text-xs font-medium text-foreground">已注册工具</div>
              <div className="mt-0.5 text-[11px] text-muted-foreground">
                MCP 标准接口，可被任何支持 MCP 的客户端调用
              </div>
            </div>
            <Badge variant="muted" className="shrink-0 font-mono">
              {tools.length} 个
            </Badge>
          </div>
          <div className="flex items-start gap-3 border-t border-border/60 px-1 pt-3 sm:border-l sm:border-t-0 sm:pl-4 sm:pt-0">
            <IconTile className="mt-0.5" icon={<FileJson className="h-3.5 w-3.5" />} />
            <div className="min-w-0 flex-1">
              <div className="text-xs font-medium text-foreground">传输协议</div>
              <div className="mt-0.5 text-[11px] text-muted-foreground">
                Streamable HTTP，默认只监听本机回环地址
              </div>
            </div>
            <Badge variant="muted" className="shrink-0 font-mono">
              HTTP
            </Badge>
          </div>
        </div>
      </SettingsCategory>

      {/* 监听配置是下方片段的**来源**：其 host/port/path 被拼进那个 URL。 */}
      <McpHttpConfigSection />

      {/* ----- 客户端配置 ---------------------------------- */}
      {/* 一格一种格式：由 daemon **实时**端口生成的 Claude Code HTTP 配置。没有传输方式
          分页，也没有客户端选择器 —— HTTP 不需要在客户端机器上装任何东西，且是 daemon
          换了端口后仍然有效的形态。见 `src/lib/mcp-client-config.ts`。 */}
      <SettingsCategory
        title="接入配置"
        description="可直接粘贴到客户端的 Claude Code 配置；HTTP 方式无需在客户端安装二进制"
        icon={<FileJson className="h-3.5 w-3.5" />}
      >
        <div className="space-y-3 p-3">
          {MCP_CLIENT_PRESETS.map((p) => {
            const configJson = buildClientConfig(p, facts);
            const reason = unavailableReason(p, facts);
            return (
              <CopyConfigTile
                key={p.id}
                icon={<FileJson className="h-4 w-4" />}
                // 不给客户端标题：只支持一种格式时，格子自己的名字只会重复卡片标题。
                description={configJson ? `${p.description} · ${p.configPath}` : reason}
                configJson={configJson ?? `// ${reason}`}
              />
            );
          })}
        </div>
      </SettingsCategory>

      {/* ----- AI 权限：两道闸，一道层级 ---------- */}
      {/* `resolve_capability` 取两层中**更严格**者，故两者不是并列开关：全局策略定上限，
          按项目授权只能收紧。摆成两张相邻卡片会让人读成「任一层单独就够」，而那永远不成立。 */}
      <SettingsCategory
        title="AI 权限"
        description="两层取更严格者：全局策略定上限，按项目授权只能收紧、不能放宽"
        icon={<Bot className="h-3.5 w-3.5" />}
        meta={
          capability === null
            ? '加载中…'
            : `${capability === 'read_only' ? '只读' : capability === 'safe_write' ? '读写' : '全部'} · ${projects.length} 个项目`
        }
      >
        {/* 上位 —— 闸 A：AI 客户端能看到哪些工具。 */}
        <PermissionLevel step={1} title="全局能力策略" hint="对所有 AI 客户端生效">
          <div className="space-y-3">
            <CapabilityRadioGroup
              value={capability}
              disabled={capabilityBusy || capability === null}
              onChange={handleCapabilityChange}
            />
            {capability !== null && (
              // 只报数量。名字在下方的卡片里已列过一次，在这里再抄前八个，只是把同一份
              // 清单换个格式重说一遍。
              <div className="text-[11px] text-muted-foreground">
                当前可见 {capabilityTools.length} 个工具
              </div>
            )}
          </div>
        </PermissionLevel>

        {/* 下位 —— 闸 B：agent 在某个项目上能做什么。 */}
        <PermissionLevel step={2} title="按项目授权" hint="只在上一层放行的范围内进一步收紧">
          <div className="space-y-3">
            <div className="text-[11px] text-muted-foreground">
              即使全局策略选了「全部」，某个项目在这里保持默认的「读+调用」时，
              该项目上的写操作依然被拒。两层中更严格的一层决定最终结果。
            </div>
            <AgentPermissionsTable projects={projects} />
          </div>
        </PermissionLevel>
      </SettingsCategory>

      {/* ----- daemon 暴露的工具集 --------------------- */}
      {/* 按名字前缀分组而非平铺：名字分七个族，一整块 `<pre>` 会把这点完全盖住。 */}
      <SettingsCategory
        title="可用工具清单"
        description={
          tools.length > 0
            ? `由 daemon 实时上报 · ${toolGroups.length} 个分类`
            : 'daemon 未上报工具清单，重启 MCP 后会刷新'
        }
        icon={<FileJson className="h-3.5 w-3.5" />}
        // 本 tab 的最后一张卡片：不带尾部横线，否则整节结束在一行后面什么都没有的位置。
        hideDivider
      >
        {toolGroups.length > 0 ? (
          <div className="space-y-3 p-3">
            {toolGroups.map((group) => (
              <div key={group.key}>
                <div className="mb-1 flex items-baseline gap-2">
                  <span className="text-[11px] font-medium text-foreground/80">{group.label}</span>
                  <span className="text-[10px] tabular-nums text-muted-foreground/70">
                    {group.items.length}
                  </span>
                </div>
                <div className="grid grid-cols-1 gap-x-4 gap-y-0.5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
                  {group.items.map((name) => (
                    <span
                      key={name}
                      title={name}
                      className="truncate rounded px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground transition-colors hover:bg-accent/60 hover:text-foreground"
                    >
                      {name}
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </div>
        ) : (
          <p className="px-3 py-6 text-center text-[11px] text-muted-foreground">
            daemon 未就绪 — 启动 MCP 后此处会列出全部已注册工具
          </p>
        )}
      </SettingsCategory>

      <DaemonRestartDialog
        open={showRestartDialog}
        onClose={() => {
          // 记下刚关闭时的状态，免得下一次 3s 轮询又用同一个状态把对话框弹回来。
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

// ----- 权限层级 --------------------------------------
// AI 权限梯子的一级。两道闸不是并列设置，故视觉语法须自己表达出这层从属关系：
//
//   - step 1 用强调色徽标 —— 它是上限。
//   - step 2 用弱化徽标并带上边框 —— 它是上一层的收紧。
//
// 不读任何说明文字的人也应看得出 step 2 在 step 1 之下而非与之并列。
//
// 两级共用同一条左边界：step 2 再嵌一层缩进，会让「step 2 属于某个东西」的说法与它
// 正文的额外位移并存，眼睛对「块从哪开始」就有两个答案。层级由分隔线、徽标色与标题
// 字重承担，几何保持平直，便于读者沿一列直扫。
//
// 正文的左 padding 由表头导出而非独立选择：表头是 `h-5` 徽标（20px）加 `gap-3`
// （12px），其**文字列**位于行自身 padding 右侧 32px 处，正文必须与标题同一起点 ——
// 从行自身 padding 起会让正文比它所属的标题左 32px，而 `AgentPermissionsTable` 自身的
// `px-3` 又把项目名推回去，一张卡就出现三条左边界。
// `p-3`（12）+ 轨距（32）= 44 = `pl-11`，两级皆同。
//
// 缩进用行**内部**的 padding 表达，绝不用外层 wrapper 的 margin：margin 落在带
// `border-t` 的元素上会连分隔线一起挪动，而子元素的 padding 不会。
function PermissionLevel({
  step,
  title,
  hint,
  meta,
  children,
}: {
  step: 1 | 2;
  title: string;
  hint: string;
  meta?: string;
  children: ReactNode;
}) {
  const subordinate = step === 2;
  return (
    <div className={cn(subordinate && 'border-t border-border/60')}>
      <div className="flex items-start gap-3 p-3">
        <span
          className={cn(
            'mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-[10px] font-semibold',
            subordinate ? 'bg-muted text-muted-foreground' : 'bg-primary text-primary-foreground',
          )}
        >
          {step}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <span
              className={cn(
                'text-xs font-medium',
                subordinate ? 'text-foreground/90' : 'text-foreground',
              )}
            >
              {title}
            </span>
            {meta && <span className="text-[10px] text-muted-foreground/80">{meta}</span>}
          </div>
          <div className="mt-0.5 text-[11px] text-muted-foreground">{hint}</div>
        </div>
      </div>
      {/* `pb-3` 两级都要：step 1 免得控件紧贴分隔线，step 2 是卡片底边所需。 */}
      <div className="pb-3 pl-11 pr-3">{children}</div>
    </div>
  );
}

// ----- 能力分段控件 ----------------------------
//
// 三个选项互斥，且形状就是下方 step 2 的按项目选择器，故用同一个分段控件：一条带边框的
// 轨道、选中项填充。逐项各画一条边框会在「AI 权限」卡片里套出三张卡片 —— 正是让这个
// tab 读成盒中盒的层次。
//
// 每项的说明放在控件下方一行即可 —— 用户只读自己选中的那一项说明。
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
    <div className="space-y-2">
      <div
        role="radiogroup"
        aria-label="全局能力策略"
        className={cn(
          'inline-flex rounded-md border border-border/70 bg-muted/40 p-0.5',
          disabled && 'opacity-50',
        )}
      >
        {CAPABILITY_OPTIONS.map((opt) => {
          const active = value === opt.value;
          return (
            <button
              key={opt.value}
              type="button"
              role="radio"
              aria-checked={active}
              disabled={disabled}
              onClick={() => onChange(opt.value)}
              className={cn(
                'inline-flex h-7 items-center rounded-sm px-3 text-[11px] font-medium transition-colors',
                focusRing,
                'disabled:cursor-not-allowed',
                active
                  ? 'bg-primary text-primary-foreground'
                  : 'text-muted-foreground hover:bg-accent hover:text-foreground',
              )}
            >
              {opt.label}
            </button>
          );
        })}
      </div>
      <div className="text-[11px] text-muted-foreground">
        {value ? CAPABILITY_OPTIONS.find((o) => o.value === value)?.hint : '正在读取当前策略…'}
      </div>
    </div>
  );
}
