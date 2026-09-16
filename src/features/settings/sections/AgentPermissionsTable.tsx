import { useState } from 'react';
import { Tooltip } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
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

const DEFAULT_CYCLE: readonly AgentPermission[] = ['full', 'read_call', 'read'];

interface AgentPermissionsTableProps {
  projects: Project[];
}

/**
 * Per-project AI agent permission picker.
 *
 * Local-state mock for v0.1; the comment below marks the v0.2 hook to lift
 * this into the app store once MCP startup actually consumes the values.
 */
// v0.2: persist to app-store
export function AgentPermissionsTable({ projects }: AgentPermissionsTableProps) {
  const [perms, setPerms] = useState<Record<string, AgentPermission>>(() => {
    const next: Record<string, AgentPermission> = {};
    projects.forEach((p, idx) => {
      next[p.id] =
        idx < DEFAULT_CYCLE.length
          ? DEFAULT_CYCLE[idx]
          : DEFAULT_CYCLE[idx % DEFAULT_CYCLE.length];
    });
    return next;
  });

  const handleChange = (projectId: string, mode: AgentPermission) => {
    setPerms((prev) => ({ ...prev, [projectId]: mode }));
  };

  return (
    <div className="space-y-2">
      <p className="text-[11px] text-muted-foreground">
        控制 AI agent 对每个项目可执行的操作范围。重启 MCP 后生效。
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
                <span className="truncate font-mono text-xs text-foreground">
                  {project.name}
                </span>
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
                        onClick={() => handleChange(project.id, mode)}
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
