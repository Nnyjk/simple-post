/**
 * 「导出接口文档」的控制器 —— 与导入/导出**共用**勾选树，但自己走一遍快照。
 *
 * 为什么不给 `useTransfer` 加 `share` 模式：那条路的预览来自后端
 * （`interchangeExportDocument` 返回的是 OpenAPI 用的薄树，没有 docs / 参数说明 / 响应定义），
 * 要拿它渲染文档就得扩 Rust 侧的 DTO——而这份文档的硬约束恰恰是「只读已保存的定义、只写名字不写值」，
 * 那些字段只有前端 store 里有。故这里复用能复用的两件东西（`docGroups` 产出的同一份
 * `TransferNode` 形状、以及 `selection.ts` 的纯运算），另起一条 60 行的状态机，不去碰导入/导出
 * 的行为。
 *
 * 所有读取都发生在**动作那一刻**，而且定义是从持久化结果重新取的（`nodes_list` /
 * `endpoint_details_list`），不是 store 里那份：`updateEndpointDetail` 先改 store 再等后端，
 * 失败才回滚，快照若取自 store 就可能冻结一次从未落库的编辑。未保存草稿本来就不在
 * `endpointDetails` 里（它们在 UI 切片的 `endpointDrafts`），因此进不了文档——这正是界面上
 * 声明的那句话。
 */
import { useCallback, useMemo, useState } from 'react';
import { useAppStore } from '@/stores/app-store';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { errorText } from '@/lib/error-text';
import { saveGeneratedDocument } from '@/lib/transfer-file-save';
import type { EndpointDetail, Environment, ResourceNode } from '@/types/domain';
import type { TransferNode } from '@/lib/interchange-types';
import { buildApiDocModel, docGroups } from '@/lib/doc/model';
import { docFileName } from '@/lib/doc/file-name';
import {
  allKeys,
  countSelected,
  invert,
  selectAll,
  selectNone,
  toggleEndpoint,
  toggleGroup,
} from '@/features/interchange/selection';

/** 导出目标：一个模块（与导入/导出同一粒度）。 */
export interface DocsShareTarget {
  projectId: string;
  moduleId: string;
  moduleName: string;
}

export interface DocsArtifact {
  html: string;
  fileName: string;
  endpoints: number;
}

export interface DocsShareController {
  target: DocsShareTarget | null;
  /** `select`：挑接口；`preview`：只读预览 + 保存。 */
  phase: 'select' | 'preview';
  groups: TransferNode[];
  selected: ReadonlySet<string>;
  chosen: number;
  total: number;
  busy: boolean;
  error: string | null;
  doc: DocsArtifact | null;
  /** 已写盘的那份文件名；`null` 表示这次会话还没保存过。 */
  savedName: string | null;
  open: (target: DocsShareTarget) => void;
  close: () => void;
  back: () => void;
  selectAll: () => void;
  selectNone: () => void;
  invert: () => void;
  toggleGroup: (node: TransferNode) => void;
  toggleEndpoint: (key: string) => void;
  generate: () => Promise<void>;
  save: () => Promise<void>;
}

export function useDocsShare(): DocsShareController {
  const [target, setTarget] = useState<DocsShareTarget | null>(null);
  const [groups, setGroups] = useState<TransferNode[]>([]);
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set());
  const [phase, setPhase] = useState<'select' | 'preview'>('select');
  const [doc, setDoc] = useState<DocsArtifact | null>(null);
  const [savedName, setSavedName] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const open = useCallback((next: DocsShareTarget) => {
    const state = useAppStore.getState();
    const nextGroups = docGroups(state.nodes, state.endpointDetails, next.moduleId);
    setTarget(next);
    setGroups(nextGroups);
    // 默认全选：来这个弹窗的人通常就是要整份文档，逐组勾是例外。
    setSelected(new Set(allKeys(nextGroups)));
    setPhase('select');
    setDoc(null);
    setSavedName(null);
    setBusy(false);
    setError(null);
  }, []);

  const close = useCallback(() => {
    setTarget(null);
    setGroups([]);
    setSelected(new Set());
    setDoc(null);
    setSavedName(null);
    setError(null);
  }, []);

  const back = useCallback(() => {
    setPhase('select');
    setDoc(null);
    setSavedName(null);
    setError(null);
  }, []);

  const total = useMemo(() => allKeys(groups).length, [groups]);
  const chosen = useMemo(() => countSelected(selected, groups), [selected, groups]);

  const generate = useCallback(async () => {
    if (!target) return;
    setBusy(true);
    setError(null);
    try {
      const state = useAppStore.getState();
      const project = state.projects.find((p) => p.id === target.projectId);
      // 定义从**持久化结果**重新读一遍，而不是用 store 里那份：`updateEndpointDetail` 是乐观
      // 更新（先改 store 再等后端），保存失败时回滚。快照若取自那份，一次失败的保存也会被冻结进
      // 发出去的文件，而它从未落库。
      const [nodes, savedDetails, environments] = await Promise.all([
        invokeCmd<ResourceNode[]>(COMMANDS.nodesList),
        invokeCmd<EndpointDetail[]>(COMMANDS.endpointDetailsList),
        invokeCmd<Environment[]>(COMMANDS.environmentsList),
      ]);
      const details: Record<string, EndpointDetail> = {};
      for (const row of savedDetails) details[row.nodeId] = row;
      // 文档里只列变量名，故环境只影响「列哪些名字」；优先用用户当前选中的那个。
      const environment =
        environments.find(
          (e) => e.id === state.activeEnvironmentId && e.projectId === target.projectId,
        ) ??
        environments.find((e) => e.projectId === target.projectId) ??
        null;
      const generatedAt = new Date();
      const model = buildApiDocModel({
        projectName: project?.name ?? target.moduleName,
        moduleId: target.moduleId,
        nodes,
        details,
        selected,
        generatedAt,
        environment,
      });
      // 弹窗开着的时候用户可能在别处删了接口、或切了项目：那时模型会是空的，
      // 生成一份「0 个接口」的快照只会让人以为工具坏了。
      if (model.endpointCount === 0) {
        throw new Error('选中的接口不在已保存的定义里了（可能刚保存、还没落库），请重新选择。');
      }
      // 渲染器牵着 `marked`，故**在这里**才动态取：文档功能没被点开时，
      // 项目树那条常驻路径上不该出现 Markdown 解析器。
      const { renderApiDocHtml } = await import('@/lib/doc/render');
      setDoc({
        html: renderApiDocHtml(model),
        fileName: docFileName(model.projectName, generatedAt),
        endpoints: model.endpointCount,
      });
      setPhase('preview');
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  }, [target, selected]);

  const save = useCallback(async () => {
    if (!doc) return;
    setBusy(true);
    setError(null);
    try {
      const outcome = await saveGeneratedDocument({
        fileName: doc.fileName,
        mediaType: 'text/html',
        content: doc.html,
        diagnostics: [],
      });
      if (outcome.status === 'saved') setSavedName(outcome.fileName);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  }, [doc]);

  return {
    target,
    phase,
    groups,
    selected,
    chosen,
    total,
    busy,
    error,
    doc,
    savedName,
    open,
    close,
    back,
    selectAll: () => setSelected((prev) => selectAll(prev, groups)),
    selectNone: () => setSelected((prev) => selectNone(prev, groups)),
    invert: () => setSelected((prev) => invert(prev, groups)),
    toggleGroup: (node) => setSelected((prev) => toggleGroup(node, prev)),
    toggleEndpoint: (key) => setSelected((prev) => toggleEndpoint(key, prev)),
    generate,
    save,
  };
}
