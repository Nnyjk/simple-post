import { useCallback, useMemo, useState } from 'react';

import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { useAppStore } from '@/stores/app-store';
import type { Environment } from '@/types/domain';
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

/**
 * 导出预览里「这个环境下没有配服务 URL」的模块名。
 *
 * 空数组是真实答案：那个环境里每个模块都配了 URL。模块名取自**这一趟预览**，
 * 故换过环境之后它讲的是新环境下的事实。
 */
export function missingServerModules(preview: ImportPreview | ExportPreview | null): string[] {
  if (!preview || !('servers' in preview)) return [];
  return preview.servers.filter((m) => m.servers.length === 0).map((m) => m.moduleName);
}

/**
 * 提示里点名的环境。
 *
 * 按**所选**环境取名字：下拉里选的是哪个环境，提示就该说哪个环境——原先写死的
 * 「该环境」在换过环境之后指的是旧环境，说的与做的不一致。
 *
 * `selectedId` 为空（「不选环境」那一项）时没有环境可点，故不给名字。
 */
export function missingServerEnvironmentName(
  environments: Environment[],
  selectedId: string,
): string | null {
  if (!selectedId) return null;
  return environments.find((e) => e.id === selectedId)?.name ?? selectedId;
}

/**
 * 取一趟导出预览。
 *
 * `environmentId` 决定 `servers` 按哪个环境解析：空串与「没传」等价，服务端那侧
 * 都退回激活环境——「不选环境」那一项送来的就是空串。
 */
export async function fetchExportPreview(args: {
  projectId: string;
  moduleId: string;
  environmentId: string;
}): Promise<ExportPreview> {
  return invokeCmd<ExportPreview>(COMMANDS.interchangeExportPreview, {
    projectId: args.projectId,
    moduleId: args.moduleId,
    environmentId: args.environmentId,
  });
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
  // 默认开着：文档带了 host 却不落成服务，用户打开模块看到的是一排没有地址的路径。
  // 取消它是一次明确取舍，不是默认值 —— 故这是预览里的一个勾，不是隐藏的默认。
  const [applyService, setApplyService] = useState(true);

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

  /** 导出预览里缺服务 URL 的模块。导入模式恒为空。 */
  const missingServers: string[] = useMemo(() => missingServerModules(preview), [preview]);

  const reset = useCallback(() => {
    setPhase('source');
    setError(null);
    setPreview(null);
    setSelected(new Set());
    setSourceText('');
    setResult(null);
    setApplyService(true);
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
        // 此刻**还没选环境**，故不送 `environmentId`：服务端按激活环境解析，返回值
        // 里的 `environmentId` 正是下拉里该显示的那一个。
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

  /**
   * 换导出环境。
   *
   * 环境一变，「哪些模块没有服务 URL」就是**那个环境**下的事实，故要重取一趟预览：
   * 沿用打开弹窗时那一趟（按激活环境算的）会把已经配好 URL 的模块点成缺 URL，
   * 真正缺 URL 的模块反而不被点名。
   *
   * 勾选**不重置**：勾选集的成员是节点 id，换环境不产生新节点，用户勾过的东西
   * 不该因为换了个环境就消失。
   */
  const changeEnvironment = useCallback(
    async (id: string) => {
      setEnvironmentId(id);
      if (!session || session.mode !== 'export') return;
      try {
        const p = await fetchExportPreview({
          projectId: session.projectId,
          moduleId: session.moduleId,
          environmentId: id,
        });
        setPreview(p);
      } catch (e) {
        setError(errorText(e));
      }
    },
    [session],
  );

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
        applyService,
      });
      setResult(r);
      setPhase('done');
      // 导入在服务端建服务，而项目行只在 bootstrap 第一层加载过一次——不刷新就看不见它。
      await useAppStore.getState().refreshProjects();
      await options.onImported?.(session.moduleId, r.createdCollectionIds);
    } catch (e) {
      setError(errorText(e));
      setPhase('preview');
    }
  }, [session, sourceText, selected, applyService, options]);

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
    applyService,
    /** 导出预览里缺服务 URL 的模块名。导入模式恒为空数组。 */
    missingServers,

    openImport,
    openExport,
    close,
    previewFromContent,
    previewFromUrl,
    commit,
    save,

    changeEnvironment,
    setFormat,
    setApplyService,
    selectAll: useCallback(() => setSelected((s) => selectAll(s, groups)), [groups]),
    selectNone: useCallback(() => setSelected((s) => selectNone(s, groups)), [groups]),
    invert: useCallback(() => setSelected((s) => invert(s, groups)), [groups]),
    toggleGroup: useCallback((node: TransferNode) => setSelected((s) => toggleGroup(node, s)), []),
    toggleEndpoint: useCallback((key: string) => setSelected((s) => toggleEndpoint(key, s)), []),
  };
}

export type TransferController = ReturnType<typeof useTransfer>;
