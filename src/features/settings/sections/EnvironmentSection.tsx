import { useEffect, useState } from 'react';
import { ChevronDown, ChevronRight, Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';
import { SettingsCategory } from '../SettingsCategory';
import type { Environment } from '@/types/domain';

export interface EnvironmentSectionProps {
  environments: Environment[];
  setEnvironmentVar: (envId: string, key: string, value: string) => void;
  addEnvironment: (projectId: string, name: string) => void;
  projectId: string;
}

const VAR_KEY_REGEX = /^[A-Za-z_][A-Za-z0-9_]*$/;

/**
 * Environment cards. Each card collapses to "name + active badge + N vars"
 * and expands to an editable key/value list. The active environment is open
 * by default; the user can toggle any card via its header button.
 */
export function EnvironmentSection({
  environments,
  setEnvironmentVar,
  addEnvironment,
  projectId,
}: EnvironmentSectionProps) {
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());

  // Auto-open the active env whenever the list shape changes. Using an
  // effect (rather than lazy init) keeps the rule in sync with the store.
  useEffect(() => {
    const active = environments.find((e) => e.isActive);
    if (!active) return;
    setExpanded((prev) => {
      if (prev.has(active.id)) return prev;
      const next = new Set(prev);
      next.add(active.id);
      return next;
    });
  }, [environments]);

  const toggle = (id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const handleAddVar = (envId: string) => {
    if (typeof window === 'undefined') return;
    const raw = window.prompt('新变量名（仅字母数字下划线）');
    if (!raw) return;
    const key = raw.trim();
    if (!VAR_KEY_REGEX.test(key)) {
      window.alert('变量名只能包含字母、数字和下划线，且不能以数字开头');
      return;
    }
    setEnvironmentVar(envId, key, '');
  };

  const handleNewEnv = () => {
    addEnvironment(projectId, 'new-env');
  };

  return (
    // No first-level title — the left rail already labels this as
    // "环境". The SettingsCategory wrapper exists only to give the
    // section a consistent top spacing + bottom padding.
    <SettingsCategory hideDivider>
      <div className="max-w-3xl space-y-3">
      {environments.length === 0 && (
        <div className="rounded-md border border-dashed border-border bg-card/30 px-3 py-6 text-center text-xs text-muted-foreground">
          该项目还没有环境
        </div>
      )}
      {environments.map((env) => {
        const isOpen = expanded.has(env.id);
        const varCount = Object.keys(env.variables).length;
        return (
          <div
            key={env.id}
            className="rounded-md border border-border bg-card/30"
          >
            <button
              type="button"
              onClick={() => toggle(env.id)}
              aria-expanded={isOpen}
              className={cn(
                'flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left',
                'hover:bg-accent/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
              )}
            >
              {isOpen ? (
                <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
              ) : (
                <ChevronRight className="h-3.5 w-3.5 text-muted-foreground" />
              )}
              <span className="font-mono text-xs font-semibold text-foreground">
                {env.name}
              </span>
              {env.isActive && <Badge variant="success">active</Badge>}
              <span className="ml-auto text-[10px] text-muted-foreground">
                {varCount} 个变量
              </span>
            </button>
            {isOpen && (
              <div className="border-t border-border/60">
                {varCount === 0 ? (
                  <div className="px-3 py-2 text-[11px] text-muted-foreground">
                    暂无变量
                  </div>
                ) : (
                  <div className="divide-y divide-border/40">
                    {Object.entries(env.variables).map(([k, v]) => (
                      <div
                        key={k}
                        className="grid grid-cols-[140px_minmax(0,1fr)] items-center gap-2 px-3 py-1.5"
                      >
                        <code className="truncate font-mono text-[11px] text-blue-400">
                          {`{{${k}}}`}
                        </code>
                        <Input
                          value={v}
                          onChange={(e) =>
                            setEnvironmentVar(env.id, k, e.target.value)
                          }
                          className="h-7 max-w-md font-mono text-xs"
                          spellCheck={false}
                        />
                      </div>
                    ))}
                  </div>
                )}
                <div className="border-t border-border/40 px-3 py-1.5">
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-6 gap-1 text-[10px] text-muted-foreground"
                    onClick={() => handleAddVar(env.id)}
                  >
                    <Plus className="h-3 w-3" />
                    添加变量
                  </Button>
                </div>
              </div>
            )}
          </div>
        );
      })}
      <Button
        variant="outline"
        size="sm"
        className="gap-1.5"
        onClick={handleNewEnv}
      >
        <Plus className="h-3.5 w-3.5" />
        新建环境
      </Button>
      </div>
    </SettingsCategory>
  );
}
