import { useState } from 'react';

/** Identifies the row a delete-confirmation dialog is pending on. */
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
 * Confirmation-dialog state for the environment section: which BaseUrl
 * definition / environment is pending deletion, plus the confirm +
 * dismiss handlers the shell wires into `ConfirmDialog`.
 *
 * Split out of `EnvironmentSection` so the shell reads as layout only —
 * the state itself is props-driven and touches no store.
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
