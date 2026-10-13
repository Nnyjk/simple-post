import { Cpu, FileJson, Bot, Server, Activity } from 'lucide-react';
import { useEffect, useState, type ReactNode } from 'react';
import { useAppStore } from '@/stores/app-store';
import { SettingsCategory } from '../SettingsCategory';
import { Badge } from '@/components/ui/badge';
import { IconTile } from '@/components/ui/icon-tile';
import { Switch } from '@/components/ui/switch';
import { McpStatusPanel, StateSummary, type McpRuntimeStatus } from './McpStatusPanel';
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
  // Selector: raw ref only — see app-store rules.
  const projects = useAppStore((s) => s.projects);

  // Subscribe to the shared MCP daemon snapshot. `useMcpStatus` is
  // the *only* place polling is started — any other consumer (TopBar
  // pill, etc.) just reads `mcpStatus` and gets the same payload.
  const mcpStatus = useAppStore((s) => s.mcpStatus);
  // The hero tile reports the daemon's lifecycle. It used to carry the
  // fixed `connection` concept hue (rose, 355°) instead, which meant a
  // daemon serving traffic painted red next to a green `running` badge.
  // See `src/lib/mcp-status-tone.ts`.
  const daemonStatus: McpRuntimeStatus = mcpStatus?.status ?? 'stopped';

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

  // ----- Generated client configs -----
  // Driven by LIVE daemon facts rather than a hand-written constant, so
  // the snippet always names the port the daemon is actually on.
  const facts = { port: mcpStatus?.port ?? null };

  const tools = mcpStatus?.registeredTools ?? [];
  // Categories for the tool table. Recomputed per render rather than
  // memoised: the input is a 44-element array and `groupTools` is a
  // single pass, so a `useMemo` would cost more than it saves.
  const toolGroups = groupTools(tools);

  return (
    <>
      {/* =================================================================
          Five cards in the order the reader actually needs them: run the
          daemon → point it at an address → hand a client the config →
          decide what that client may do → see the resulting toolset.

          There are no section headings. This tab carried three numbered
          ones (① 服务 / ② 接入 / ③ 权限) and every one of them restated
          the card directly beneath it — ① 服务 sat above a card titled
          「MCP 进程」, ② 接入 above 「HTTP 服务配置」. A heading that
          repeats its own card's title is one more line to read and no
          new information, so the cards carry the structure themselves
          and the separation falls to `SettingsCategory`'s own bottom
          border.
          ================================================================= */}

      {/* ----- The daemon, and the only card that runs it --------- */}
      <SettingsCategory
        title="MCP 进程"
        description="MCP 守护进程 · 以 Streamable HTTP 暴露给 AI 客户端"
        icon={<Server className="h-3.5 w-3.5" />}
        meta="进程级状态"
      >
        <div className="flex flex-col gap-4 p-4 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-start gap-3">
            <IconTile
              // Status, not identity: see the note on `daemonStatus`.
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

        {/* Auto-start used to be a whole second card. It is one switch
            about the same daemon the buttons above already control, so
            it now sits directly under them instead of competing with
            them from a card away. */}
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

      {/* The listener config is the SOURCE of the snippet below it: its
          host/port/path are concatenated into that URL. */}
      <McpHttpConfigSection />

      {/* ----- Client-side config ---------------------------------- */}
      {/* One tile, one format: the Claude Code HTTP shape generated from
          the LIVE daemon port. There is no transport tab and no client
          picker — HTTP needs nothing installed on the client machine,
          and it is the shape that keeps working after the daemon picks a
          new port. See `src/lib/mcp-client-config.ts`. */}
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
                // No client title: with one supported format the tile's
                // own name would just repeat the card header.
                description={configJson ? `${p.description} · ${p.configPath}` : reason}
                configJson={configJson ?? `// ${reason}`}
              />
            );
          })}
        </div>
      </SettingsCategory>

      {/* ----- AI permissions: two gates, one hierarchy ---------- */}
      {/* `resolve_capability` merges these by taking the STRICTER of the
          two, so they are not two parallel switches: the global policy
          sets the ceiling and a per-project grant can only tighten it.
          Rendering them as two adjacent cards invited the reading that
          either one alone is sufficient, which is the one reading that
          is never true. */}
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
        {/* 上位 — Gate A: which tools an AI client can even see. */}
        <PermissionLevel step={1} title="全局能力策略" hint="对所有 AI 客户端生效">
          <div className="space-y-3">
            <CapabilityRadioGroup
              value={capability}
              disabled={capabilityBusy || capability === null}
              onChange={handleCapabilityChange}
            />
            {capability !== null && (
              // Count only. The names appear once, in the card below —
              // spelling out the first eight here repeated half of that
              // list in a second format for no added signal.
              <div className="text-[11px] text-muted-foreground">
                当前可见 {capabilityTools.length} 个工具
              </div>
            )}
          </div>
        </PermissionLevel>

        {/* 下位 — Gate B: what an agent may do on one project. */}
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

      {/* ----- The toolset the daemon exposes --------------------- */}
      {/* Grouped by name prefix rather than listed flat: the names come
          in seven families, and one `<pre>` of them hid that entirely.

          The 「代理二进制路径」 block that used to sit at the bottom of
          this card is gone. The proxy binary is a stdio artefact — it
          existed so a client could spawn a bridge process — and this tab
          is HTTP-only since the transport narrowed, so the path it
          printed was a file nothing here can use any more. */}
      <SettingsCategory
        title="可用工具清单"
        description={
          tools.length > 0
            ? `由 daemon 实时上报 · ${toolGroups.length} 个分类`
            : 'daemon 未上报工具清单，重启 MCP 后会刷新'
        }
        icon={<FileJson className="h-3.5 w-3.5" />}
        // Last card in the tab: no trailing rule, otherwise the section
        // ends on a line with nothing after it.
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

// ----- Permission level --------------------------------------
// One rung of the AI-permission ladder. The visual grammar has to
// carry the relationship on its own, because the two gates are not
// parallel settings:
//
//   - step 1 takes the accent-coloured badge — it is the ceiling.
//   - step 2 takes a muted badge and carries a top divider — it is a
//     narrowing of the level above it.
//
// A reader who never reads a word of the prose should still see that
// step 2 lives under step 1 rather than beside it.
//
// BOTH RUNGS SHARE ONE LEFT EDGE. That is a correction, not a stylistic
// preference: indenting step 2 was tried and is wrong here. A second
// inset stacked on the card's own padding made 「② is inside something」
// claim while its prose kept the extra shift, and the eye then had two
// different answers to "where does a block start". The hierarchy is
// carried by the divider, the badge colour and the title's weight;
// geometry stays flat so a reader can scan straight down one column.
//
// The body's left padding is derived from the header's, not chosen
// independently: the header is a `h-5` badge (20px) plus `gap-3` (12px),
// so its TEXT column sits 32px right of the row's own padding. The body
// has to start where the title does — starting at the row's own padding
// ran the prose 32px to the LEFT of the heading it belongs to, and
// `AgentPermissionsTable`'s own `px-3` pushed the project names back out
// again, which is how one card ended up with three left edges.
// `p-3` (12) + rail (32) = 44 = `pl-11`, on both rungs.
//
// The indent is expressed with padding *inside* the row, never with a
// margin on the wrapper. `sm:ml-4` used to sit on the same element as
// `border-t`, so the rule above step 2 started 16px in and left a notch
// against the step-1 block above it. A margin on the bordered element
// moves the separator; padding on its children cannot.
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
      {/* `pb-3` on BOTH rungs. Step 1 needs it so its controls do not sit
          flush against the separator; step 2 needs it for the card's
          bottom edge. The old ternary handed it to step 2 only, which
          left step 1's content touching the rule. */}
      <div className="pb-3 pl-11 pr-3">{children}</div>
    </div>
  );
}

// ----- Capability segmented control ----------------------------
//
// This was three stacked `<label>`s that each drew their own border —
// three cards nested inside the "AI 权限" card, which is the exact
// layering that made the tab read as boxes inside boxes. The three
// options are mutually exclusive and are what the per-project picker in
// step 2 below already looks like, so it is the same segmented control
// as that one: one bordered track, the active option filled.
//
// The per-option hint moved out of the option and onto a single line
// under the control, which is all it ever needed — you read about the
// one you picked, not the two you didn't.
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
