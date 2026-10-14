import { Braces, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Tooltip } from '@/components/ui/tooltip';
import type { Environment } from '@/types/domain';
import type { EnvironmentSectionProps } from '../EnvironmentSection';
import { InlineNameCell } from './InlineNameCell';
import { KeyValueTable, RowTile } from './KeyValueTable';

const VAR_KEY_REGEX = /^[A-Za-z_][A-Za-z0-9_]*$/;

/**
 * VariableTable —— 该环境的 `{{key}} → value` 覆盖值。
 *
 * 列表外壳与 前置 URL 表共用 `KeyValueTable`；只有下面的单元格是本表自己的。
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

  return (
    <KeyValueTable
      items={entries}
      keyOf={([k], idx) => k || `__empty_${idx}`}
      emptyText="暂无变量"
      addText="添加变量"
      onAdd={onAddVar}
      rowClassName="py-2.5"
      renderRow={([name, value]) => (
        <>
          <RowTile concept="variable" icon={<Braces className="h-4 w-4" />} />

          {/* 变量名 —— 就地重命名 */}
          <div className="min-w-0 w-44 shrink-0">
            <InlineNameCell
              value={name}
              placeholder="变量名"
              validate={(v) =>
                VAR_KEY_REGEX.test(v.trim()) ? null : '变量名仅允许字母数字下划线,且不能以数字开头'
              }
              onCommit={(newKey) => {
                if (!newKey || newKey === name) return;
                // 一个命令完成重命名。拆成「先删旧 key、再写新 key」两次调用时，两次之间
                // 变量处于消失状态 —— 进程中断或第二次调用失败就会把它删掉。单次原子调用后
                // 没有这个窗口。
                renameEnvironmentVar(env.id, name, newKey);
              }}
              className="font-mono text-xs font-medium"
            />
            <div className="label-caps mt-0.5">{`{{${name || '...'}}}`}</div>
          </div>

          {/* 变量值 */}
          <div className="min-w-0 flex-1">
            <Input
              value={value}
              onChange={(e) => setEnvironmentVar(env.id, name, e.target.value)}
              placeholder="值(支持 {{其它变量}} 引用)"
              className="h-8 font-mono text-xs"
              spellCheck={false}
            />
          </div>

          {/* 末尾操作 */}
          <div className="flex shrink-0 items-center gap-1">
            <Tooltip content="删除变量" side="bottom">
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-muted-foreground opacity-0 hover:text-destructive group-hover:opacity-100"
                onClick={() => setEnvironmentVar(env.id, name, null)}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </Tooltip>
          </div>
        </>
      )}
    />
  );
}
