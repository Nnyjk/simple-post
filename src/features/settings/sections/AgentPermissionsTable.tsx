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
        {/* `text-warning` is the *border* step of the amber ramp
            (38 92% 45%); on the card's own surface it lands around 3.3:1
            in the light theme, which 11px text cannot carry.
            `--warning-foreground` is the text step (28 80% 35% light,
            38 92% 75% dark) and is what the warning badge uses. Same
            rule as the tile tones — see AGENTS.md's semantic-token
            pairing note. */}
        <span className="text-warning-foreground">修改权限后需重启 MCP daemon 生效</span>。
      </p>
      {/* No border/bg shell: this already lives inside the "AI 权限"
          category card, and a second bordered surface inside it read as
          a card nested in a card. The row separators carry the grouping.

          No `px-3` either. `PermissionLevel` already indents this block to
          sit under its own heading; a second 12px inset on top of that
          pushed the project names 28px right of the 「按项目授权」 title
          they belong to, which is the misalignment this table was part
          of. Flush left, the colour dot hangs like a list bullet and the
          separator spans the full content width. */}
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
              {/* Same trough as the segmented control directly above it in the same
                card (`CapabilityRadioGroup`): h-7 segments, `border/70`
                over `muted/40`. They were h-6 over `background/50`, so
                two controls of the same kind rendered a step apart in
                size and on two different surfaces — the other half of the
                misalignment. The focus ring stays bare because this track
                is wrapped in a `Tooltip`, which takes the real outline. */}
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
 * Narrow an arbitrary string (the union persisted by the Rust side)
 * into the `AgentPermission` enum. Anything we don't recognise falls
 * back to `read_call` so the UI never lands on a phantom mode.
 */
function isPermission(v: unknown): v is AgentPermission {
  return v === 'read' || v === 'read_call' || v === 'full';
}
