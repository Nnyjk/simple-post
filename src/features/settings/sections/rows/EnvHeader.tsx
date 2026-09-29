import type { Environment } from '@/types/domain';
import { envIcon } from '../env-utils';
import { InlineNameCell } from './InlineNameCell';

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
      <span
        aria-hidden
        className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-primary/30 bg-primary/15 text-primary"
      >
        {envIcon(env.name)}
      </span>
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
