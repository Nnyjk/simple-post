import { useCallback, useMemo, useState } from 'react';

import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type {
  DocFormat,
  ExportDocument,
  ExportPreview,
  ImportCommitResult,
  ImportPreview,
  TransferDiagnostic,
  TransferNode,
} from '@/lib/interchange-types';
import { copyGeneratedDocument, saveGeneratedDocument } from '@/lib/transfer-file-save';
import {
  allKeys,
  countSelected,
  invert,
  selectAll,
  selectNone,
  toggleEndpoint,
  toggleGroup,
} from './selection';

/** 弹窗的一次会话：打开它的那一行模块。 */
export interface TransferSession {
  mode: 'import' | 'export';
  projectId: string;
  moduleId: string;
  moduleName: string;
}

/**
 * 一条边上的五个阶段。
 *
 * `source`（选文件 / 填链接）与 `preview`（勾选）是**分开**的两屏：选来源时没什么
 * 可看的，而预览可能有几百行，两者塞进一屏会让来源那三个控件被挤到看不见。
 */
export type TransferPhase = 'source' | 'working' | 'preview' | 'done';

export interface UseTransferOptions {
  /** 导入落库成功后由调用方刷新树并展开落点。 */
  onImported?: (moduleId: string, createdCollectionIds: string[]) => void | Promise<void>;
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function useTransfer(options: UseTransferOptions = {}) {
  const [session, setSession] = useState<TransferSession | null>(null);
  const [phase, setPhase] = useState<TransferPhase>('source');
  const [error, setError] = useState<string | null>(null);

  const [preview, setPreview] = useState<ImportPreview | ExportPreview | null>(null);
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());

  // 提交时要原样送回**同一份**原文，否则 URL 模式会再拉一次，且两次拉到的
  // 可能已经不是同一份文件了。
  const [sourceText, setSourceText] = useState('');
  const [result, setResult] = useState<ImportCommitResult | null>(null);
  const [environmentId, setEnvironmentId] = useState('');
  const [format, setFormat] = useState<DocFormat>('json');

  const groups: TransferNode[] = useMemo(
    () => (preview ? (preview.groups as TransferNode[]) : []),
    [preview],
  );
  const total = useMemo(() => allKeys(groups).length, [groups]);
  const chosen = useMemo(() => countSelected(selected, groups), [selected, groups]);

  const diagnostics: TransferDiagnostic[] = useMemo(
    () => (preview ? preview.diagnostics : []),
    [preview],
  );

  const reset = useCallback(() => {
    setPhase('source');
    setError(null);
    setPreview(null);
    setSelected(new Set());
    setSourceText('');
    setResult(null);
  }, []);

  const openImport = useCallback(
    (s: Omit<TransferSession, 'mode'>) => {
      setSession({ ...s, mode: 'import' });
      reset();
    },
    [reset],
  );

  const openExport = useCallback(
    async (s: Omit<TransferSession, 'mode'>) => {
      setSession({ ...s, mode: 'export' });
      reset();
      setPhase('working');
      try {
        const p = await invokeCmd<ExportPreview>(COMMANDS.interchangeExportPreview, {
          projectId: s.projectId,
          moduleId: s.moduleId,
        });
        setPreview(p);
        setSelected(new Set(allKeys(p.groups)));
        setEnvironmentId(p.environmentId ?? '');
        setPhase('preview');
      } catch (e) {
        setError(errorText(e));
        setPhase('source');
      }
    },
    [reset],
  );

  const close = useCallback(() => {
    setSession(null);
    reset();
  }, [reset]);

  /** 走预览。`text` 为空表示让它自己从链接拉。 */
  const runPreview = useCallback(
    async (args: { content?: string; url?: string }) => {
      if (!session) return;
      setPhase('working');
      setError(null);
      try {
        const p = await invokeCmd<ImportPreview>(COMMANDS.interchangeImportPreview, {
          projectId: session.projectId,
          moduleId: session.moduleId,
          ...(args.content ? { content: args.content } : {}),
          ...(args.url ? { url: args.url } : {}),
        });
        setPreview(p);
        setSelected(new Set(allKeys(p.groups)));
        // URL 模式：提交时要送回的就是这次拉到的原文，不是再拉一次。
        setSourceText(p.resolvedContent ?? args.content ?? '');
        setPhase('preview');
      } catch (e) {
        setError(errorText(e));
        setPhase('source');
      }
    },
    [session],
  );

  const previewFromContent = useCallback(
    (content: string) => runPreview({ content }),
    [runPreview],
  );

  const previewFromUrl = useCallback((url: string) => runPreview({ url }), [runPreview]);

  const commit = useCallback(async () => {
    if (!session) return;
    setPhase('working');
    setError(null);
    try {
      const r = await invokeCmd<ImportCommitResult>(COMMANDS.interchangeImportCommit, {
        projectId: session.projectId,
        moduleId: session.moduleId,
        content: sourceText,
        selected: [...selected],
      });
      setResult(r);
      setPhase('done');
      await options.onImported?.(session.moduleId, r.createdCollectionIds);
    } catch (e) {
      setError(errorText(e));
      setPhase('preview');
    }
  }, [session, sourceText, selected, options]);

  /** 产出文档并交给用户。`copy` 为真时只复制，不弹保存对话框。 */
  const save = useCallback(
    async (copy: boolean) => {
      if (!session) return;
      setPhase('working');
      setError(null);
      try {
        const doc = await invokeCmd<ExportDocument>(COMMANDS.interchangeExportDocument, {
          projectId: session.projectId,
          moduleId: session.moduleId,
          environmentId,
          selected: [...selected],
          format,
        });
        if (copy) {
          await copyGeneratedDocument(doc);
          close();
          return;
        }
        const outcome = await saveGeneratedDocument(doc);
        // 用户点了取消不是失败：回到勾选那屏，改个格式再试一次。
        if (outcome === 'cancelled') {
          setPhase('preview');
          return;
        }
        close();
      } catch (e) {
        setError(errorText(e));
        setPhase('preview');
      }
    },
    [session, environmentId, selected, format, close],
  );

  return {
    session,
    phase,
    error,
    preview,
    groups,
    diagnostics,
    total,
    chosen,
    selected,
    result,
    environmentId,
    format,

    openImport,
    openExport,
    close,
    previewFromContent,
    previewFromUrl,
    commit,
    save,

    setEnvironmentId,
    setFormat,
    selectAll: useCallback(() => setSelected((s) => selectAll(s, groups)), [groups]),
    selectNone: useCallback(() => setSelected((s) => selectNone(s, groups)), [groups]),
    invert: useCallback(() => setSelected((s) => invert(s, groups)), [groups]),
    toggleGroup: useCallback((node: TransferNode) => setSelected((s) => toggleGroup(node, s)), []),
    toggleEndpoint: useCallback((key: string) => setSelected((s) => toggleEndpoint(key, s)), []),
  };
}

export type TransferController = ReturnType<typeof useTransfer>;
