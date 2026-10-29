import { useState } from 'react';

/** 标识删除确认对话框正在等待的那一行。 */
export interface PendingDelete {
  id: string;
  name: string;
}

interface UseEnvDialogsArgs {
  deleteEnvironment: (envId: string) => void;
}

/**
 * 环境分区的确认对话框状态：哪个环境待删除，外加外壳接进 `ConfirmDialog` 的确认与
 * 取消处理器。
 *
 * 从 `EnvironmentSection` 拆出，好让外壳读起来只有布局 —— 这些状态由 props 驱动，不碰
 * store。
 */
export function useEnvDialogs({ deleteEnvironment }: UseEnvDialogsArgs) {
  const [deletingEnv, setDeletingEnv] = useState<PendingDelete | null>(null);

  const requestEnvDelete = (env: PendingDelete) => setDeletingEnv(env);

  const clearEnvDelete = () => setDeletingEnv(null);

  const confirmDeleteEnv = () => {
    if (deletingEnv) {
      deleteEnvironment(deletingEnv.id);
    }
  };

  return {
    deletingEnv,
    requestEnvDelete,
    clearEnvDelete,
    confirmDeleteEnv,
  };
}
