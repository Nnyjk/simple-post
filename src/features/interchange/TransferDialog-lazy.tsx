import { useAppStore } from '@/stores/app-store';

import { TransferDialog } from './TransferDialog';
import type { TransferController } from './useTransfer';

/**
 * 弹窗的 lazy 壳。
 *
 * 存在是因为环境列表该由 store 给，而 `ProjectTree` 不该为此知道弹窗的 props：
 * 它只握着一只 controller，弹窗自己渲染时去读环境。
 *
 * 本文件必须与 `TransferDialog` 分开——`lazy()` 的粒度是文件。
 * 把这两行合进 `TransferDialog.tsx` 只会让调用方静态引入弹窗本体，
 * 于是勾选树与诊断区又回到树的冷启动路径上。
 */
export function TransferDialogLazy({ controller }: { controller: TransferController }) {
  const environments = useAppStore((s) => s.environments);
  return (
    <TransferDialog
      controller={controller}
      environments={environments}
      onClose={controller.close}
    />
  );
}
