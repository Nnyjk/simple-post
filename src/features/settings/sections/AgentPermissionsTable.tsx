import { useEffect, useState } from 'react';
import { Tooltip } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
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
 * Per-project AI agent permission picker.
 *
 * Persists via `permissions_get` / `permissions_set` Tauri commands.
 * The table also seeds an in-memory map of pending edits so the UI
 * stays responsive while the invoke round-trips; on failure we revert
 * to the persisted value.
 */
export function AgentPermissionsTable({ projects }: AgentPermissionsTableProps) {
  const [perms, setPerms] = useState<Record<string, AgentPermission>>({});
  const [loaded, setLoaded] = useState(false);

  // Fetch persisted permissions once on mount and whenever the
  // project list changes (new project → new default row). Unknown
  // permissions default to `read_call` — the safest middle tier.
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
    // Re-fetch when the project slice's *identity* changes. The
    // original `[projects.length]` over-fired on single-field edits
    // (the count didn't change but the effect re-ran anyway because
    // `length` changed with add/remove) and missed reorders. The
    // slice reference is stable until the array mutates, so this is
    // both cheaper and more correct.
  }, [projects]);

  const handleChange = async (projectId: string, mode: AgentPermission) => {
    const prev = perms[projectId] ?? 'read_call';
    // Optimistic local update for snappy UX. Roll back on failure.
    setPerms((p) => ({ ...p, [projectId]: mode }));
    try {
      // Rust `permissions_set` returns `()` on success, so type the
      // invoke as `void`. Previous calls left the response untyped,
      // which masked the desired shape and slipped through lint.
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
        <span className="text-warning">修改权限后需重启 MCP daemon 生效</span>。
      </p>
      <div className="overflow-hidden rounded-md border border-border bg-card/30">
        {projects.map((project, idx) => {
          const current = perms[project.id] ?? 'read_call';
          return (
            <div
              key={project.id}
              className={cn(
                'grid grid-cols-[1fr_auto] items-center gap-3 px-3 py-1.5',
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
              <div
                role="radiogroup"
                aria-label={`${project.name} 权限`}
                className="inline-flex items-center gap-1 rounded-md border border-border bg-background/50 p-0.5"
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
                          'inline-flex h-6 items-center rounded-sm px-2 text-[11px] font-medium transition-colors',
                          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
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
 * Narrow an arbitrary string (the union persisted by the Rust side)
 * into the `AgentPermission` enum. Anything we don't recognise falls
 * back to `read_call` so the UI never lands on a phantom mode.
 */
function isPermission(v: unknown): v is AgentPermission {
  return v === 'read' || v === 'read_call' || v === 'full';
}
