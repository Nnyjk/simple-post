import { useState } from 'react';

/** 标识删除确认对话框正在等待的那一行。 */
export interface PendingDelete {
  id: string;
  name: string;
}

interface UseEnvDialogsArgs {
  projectId: string;
  deleteBaseUrlDefinition: (projectId: string, defId: string) => void;
  deleteEnvironment: (envId: string) => void;
}

/**
 * 环境分区的确认对话框状态：哪个 BaseUrl 定义 / 环境待删除，外加外壳接进
 * `ConfirmDialog` 的确认与取消处理器。
 *
 * 从 `EnvironmentSection` 拆出，好让外壳读起来只有布局 —— 这些状态由 props 驱动，不碰
 * store。
 */
export function useEnvDialogs({
  projectId,
  deleteBaseUrlDefinition,
  deleteEnvironment,
}: UseEnvDialogsArgs) {
  const [deletingBaseUrl, setDeletingBaseUrl] = useState<PendingDelete | null>(null);
  const [deletingEnv, setDeletingEnv] = useState<PendingDelete | null>(null);

  const requestBaseUrlDelete = (def: PendingDelete) => setDeletingBaseUrl(def);
  const requestEnvDelete = (env: PendingDelete) => setDeletingEnv(env);

  const clearBaseUrlDelete = () => setDeletingBaseUrl(null);
  const clearEnvDelete = () => setDeletingEnv(null);

  const confirmDeleteBaseUrl = () => {
    if (deletingBaseUrl) {
      deleteBaseUrlDefinition(projectId, deletingBaseUrl.id);
    }
  };

  const confirmDeleteEnv = () => {
    if (deletingEnv) {
      deleteEnvironment(deletingEnv.id);
    }
  };

  return {
    deletingBaseUrl,
    deletingEnv,
    requestBaseUrlDelete,
    requestEnvDelete,
    clearBaseUrlDelete,
    clearEnvDelete,
    confirmDeleteBaseUrl,
    confirmDeleteEnv,
  };
}
