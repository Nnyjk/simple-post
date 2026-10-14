import { useEffect, useState } from 'react';
import { Tooltip } from '@/components/ui/tooltip';
import { cn, focusRingBare } from '@/lib/utils';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { Project } from '@/types/domain';

export type AgentPermission = 'read' | 'read_call' | 'full';

const MODE_LABELS: Record<AgentPermission, { label: string; tools: string }> = {
  read: {
    label: '只读',
    tools: 'list · get',
  },
  read_call: {
    label: '读+调用',
    tools: 'list · get · api_run_request',
  },
  full: {
    label: '完全操作',
    tools: 'list · get · api_run_request · create · update · delete',
  },
};

interface AgentPermissionsTableProps {
  projects: Project[];
}

/**
 * 按项目的 AI agent 权限选择器。
 *
 * 经 `permissions_get` / `permissions_set` 持久化。表内另存一份待写入 map，让 invoke
 * 往返期间界面仍然跟手；失败则回落到已持久化的值。
 */
export function AgentPermissionsTable({ projects }: AgentPermissionsTableProps) {
  const [perms, setPerms] = useState<Record<string, AgentPermission>>({});
  const [loaded, setLoaded] = useState(false);

  // 挂载时取一次已持久化的权限，项目列表变化时再取（新项目即新的一行）。认不出的权限值
  // 落到 `read_call`，即最稳妥的中间档。
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const out: Record<string, AgentPermission> = {};
      await Promise.all(
        projects.map(async (p) => {
          try {
            const v = await invokeCmd<string>(COMMANDS.permissionsGet, { projectId: p.id });
            if (isPermission(v)) {
              out[p.id] = v;
            } else {
              out[p.id] = 'read_call';
            }
          } catch (e) {
            console.error('[permissions] permissions_get failed', e);
            out[p.id] = 'read_call';
          }
        }),
      );
      if (!cancelled) {
        setPerms(out);
        setLoaded(true);
      }
    })();
    return () => {
      cancelled = true;
    };
    // 依赖项目 slice 的*引用*而非长度：数组没变就无需重取，变了就该重取
    // （增删与重排都靠引用变化捕捉到）。
  }, [projects]);

  const handleChange = async (projectId: string, mode: AgentPermission) => {
    const prev = perms[projectId] ?? 'read_call';
    // 乐观更新保证跟手，失败回滚。
    setPerms((p) => ({ ...p, [projectId]: mode }));
    try {
      // Rust 侧 `permissions_set` 成功时返回 `()`，故把这次 invoke 标成 `void`。
      await invokeCmd<void>(COMMANDS.permissionsSet, { projectId, permission: mode });
    } catch (e) {
      console.error('[permissions] permissions_set failed', e);
      setPerms((p) => ({ ...p, [projectId]: prev }));
    }
  };

  if (!loaded) {
    return (
      <div className="space-y-2">
        <p className="text-[11px] text-muted-foreground">读取 agent 权限…</p>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <p className="text-[11px] text-muted-foreground">
        控制 AI agent 对每个项目可执行的操作范围。
        {/* `text-warning` 是琥珀色阶的*边框*档（38 92% 45%），压在卡片自身表面上，
            亮色主题下约 3.3:1，11px 文字撑不住。`--warning-foreground` 才是文字档
            （亮 28 80% 35%、暗 38 92% 75%），也是警告徽标在用的那一个。规则同色块
            那组，见 AGENTS.md 的语义 token 配对一节。 */}
        <span className="text-warning-foreground">修改权限后需重启 MCP daemon 生效</span>。
      </p>
      {/* 不加边框和底色：本区块已经位于「AI 权限」分类卡片内，再套一层带边框的表面会
          读成卡片套卡片。分组交给行分隔线。

          也不加 `px-3`：`PermissionLevel` 已把本块缩进到自己的标题之下，再叠 12px 会把
          项目名推到离「按项目授权」标题 28px 的位置。齐左后色点像列表符号挂着，分隔线也
          正好横跨整个内容宽度。 */}
      <div>
        {projects.map((project, idx) => {
          const current = perms[project.id] ?? 'read_call';
          return (
            <div
              key={project.id}
              className={cn(
                'grid grid-cols-[1fr_auto] items-center gap-3 py-1.5',
                idx > 0 && 'border-t border-border/60',
              )}
            >
              <div className="flex min-w-0 items-center gap-2">
                <span
                  className="inline-block h-2 w-2 shrink-0 rounded-full"
                  style={{ backgroundColor: project.color }}
                  aria-hidden
                />
                <span className="truncate font-mono text-xs text-foreground">{project.name}</span>
              </div>
              {/* 与同一张卡片里正上方的分段控件（`CapabilityRadioGroup`）用同一条凹槽：
                  h-7 分段、`border/70` 配 `muted/40`。本轨道用裸 focus ring，因为它包在
                  `Tooltip` 里，真正的外框由 Tooltip 给出。 */}
              <div
                role="radiogroup"
                aria-label={`${project.name} 权限`}
                className="inline-flex items-center gap-1 rounded-md border border-border/70 bg-muted/40 p-0.5"
              >
                {(Object.keys(MODE_LABELS) as AgentPermission[]).map((mode) => {
                  const active = current === mode;
                  return (
                    <Tooltip key={mode} content={MODE_LABELS[mode].tools}>
                      <button
                        type="button"
                        role="radio"
                        aria-checked={active}
                        onClick={() => {
                          void handleChange(project.id, mode);
                        }}
                        className={cn(
                          'inline-flex h-7 items-center rounded-sm px-3 text-[11px] font-medium transition-colors',
                          focusRingBare,
                          active
                            ? 'bg-primary text-primary-foreground'
                            : 'text-muted-foreground hover:bg-accent hover:text-foreground',
                        )}
                      >
                        {MODE_LABELS[mode].label}
                      </button>
                    </Tooltip>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/**
 * 把任意字符串（Rust 侧持久化的联合类型）收窄成 `AgentPermission` 枚举。认不出的都
 * 落回 `read_call`，界面上不会出现幻影档位。
 */
function isPermission(v: unknown): v is AgentPermission {
  return v === 'read' || v === 'read_call' || v === 'full';
}
