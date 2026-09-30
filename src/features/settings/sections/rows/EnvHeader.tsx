import type { Environment } from '@/types/domain';
import { envIcon } from '../env-utils';
import { InlineNameCell } from './InlineNameCell';
import { IconTile } from '@/components/ui/icon-tile';

/**
 * EnvHeader — env name (inline rename).
 */
export function EnvHeader({
  env,
  onRename,
}: {
  env: Environment;
  onRename: (name: string) => void;
}) {
  return (
    <div className="flex items-center gap-2 border-b border-border/60 px-4 py-2.5">
      <IconTile size="md" tone="primary" bordered icon={envIcon(env.name)} />
      <div className="min-w-0">
        <InlineNameCell
          value={env.name}
          placeholder="环境名"
          onCommit={onRename}
          className="text-sm font-semibold"
        />
      </div>
    </div>
  );
}
