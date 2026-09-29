import { Braces, Plus, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Tooltip } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import type { Environment } from '@/types/domain';
import type { EnvironmentSectionProps } from '../EnvironmentSection';
import { InlineNameCell } from './InlineNameCell';

const VAR_KEY_REGEX = /^[A-Za-z_][A-Za-z0-9_]*$/;

/**
 * VariableTable — the env's `{{key}} → value` overrides.
 */
interface VariableTableProps {
  env: Environment;
  setEnvironmentVar: EnvironmentSectionProps['setEnvironmentVar'];
  renameEnvironmentVar: EnvironmentSectionProps['renameEnvironmentVar'];
  onAddVar: () => void;
}

export function VariableTable({
  env,
  setEnvironmentVar,
  renameEnvironmentVar,
  onAddVar,
}: VariableTableProps) {
  const entries = Object.entries(env.variables);

  if (entries.length === 0) {
    return (
      <div className="flex flex-col items-center gap-2 px-4 py-6 text-center">
        <div className="text-[11px] text-muted-foreground">暂无变量</div>
        <Button variant="default" size="sm" className="h-7 gap-1 text-xs" onClick={onAddVar}>
          <Plus className="h-3.5 w-3.5" />
          添加变量
        </Button>
      </div>
    );
  }

  return (
    <div className="flex flex-col">
      {entries.map(([k, v], idx) => (
        <VariableRow
          key={k || `__empty_${idx}`}
          flush={idx === 0}
          name={k}
          value={v}
          envId={env.id}
          setEnvironmentVar={setEnvironmentVar}
          renameEnvironmentVar={renameEnvironmentVar}
        />
      ))}
      <button
        type="button"
        onClick={onAddVar}
        className="flex items-center gap-2 border-t border-border/60 px-4 py-2 text-[11px] text-muted-foreground transition-colors hover:bg-accent/40 hover:text-foreground"
      >
        <Plus className="h-3.5 w-3.5" />
        添加变量
      </button>
    </div>
  );
}

interface VariableRowProps {
  flush: boolean;
  name: string;
  value: string;
  envId: string;
  setEnvironmentVar: VariableTableProps['setEnvironmentVar'];
  renameEnvironmentVar: VariableTableProps['renameEnvironmentVar'];
}

function VariableRow({
  flush,
  name,
  value,
  envId,
  setEnvironmentVar,
  renameEnvironmentVar,
}: VariableRowProps) {
  return (
    <div
      className={cn(
        'group flex items-start gap-3 px-4 py-2.5',
        !flush && 'border-t border-border/60',
      )}
    >
      {/* Icon container — amber for "config"-shaped rows */}
      <span
        aria-hidden
        className="mt-0.5 inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-amber-500/30 bg-amber-500/15 text-amber-500"
      >
        <Braces className="h-4 w-4" />
      </span>

      {/* Variable key — inline rename */}
      <div className="min-w-0 w-44 shrink-0">
        <InlineNameCell
          value={name}
          placeholder="变量名"
          validate={(v) =>
            VAR_KEY_REGEX.test(v.trim()) ? null : '变量名仅允许字母数字下划线,且不能以数字开头'
          }
          onCommit={(newKey) => {
            if (!newKey || newKey === name) return;
            // 一个命令完成重命名。之前这里是「先删旧 key，再写新 key」
            // 两次 setEnvironmentVar，两次事务之间变量是消失的——进程
            // 中断或第二次调用失败就会把变量删掉。改成单次原子调用后
            // 不存在这个窗口。
            renameEnvironmentVar(envId, name, newKey);
          }}
          className="font-mono text-xs font-medium"
        />
        <div className="mt-0.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
          {`{{${name || '...'}}}`}
        </div>
      </div>

      {/* Variable value */}
      <div className="min-w-0 flex-1">
        <Input
          value={value}
          onChange={(e) => setEnvironmentVar(envId, name, e.target.value)}
          placeholder="值(支持 {{其它变量}} 引用)"
          className="h-8 font-mono text-xs"
          spellCheck={false}
        />
      </div>

      {/* Trailing actions */}
      <div className="flex shrink-0 items-center gap-1">
        <Tooltip content="删除变量" side="bottom">
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 text-muted-foreground opacity-0 hover:text-destructive group-hover:opacity-100"
            onClick={() => setEnvironmentVar(envId, name, null)}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </Tooltip>
      </div>
    </div>
  );
}
