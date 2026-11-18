import { useEffect, useMemo, useRef, useState } from 'react';
import { GraphQlEditor } from './GraphQlEditor-lazy';
import type { GraphQlCompletionSource } from './GraphQlEditor';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { variablesProblem } from '@/lib/graphql';
import { completionCandidates, type GraphQlSchema } from '@/lib/graphql-schema';
import type { GraphQlSchemaStatus } from './graphql/use-graphql-schema';
import type { QueryHistoryEntry } from './graphql/query-history';
import type { GraphQlRequest } from '@/types/domain';

/** 面板顶部那一行要的 schema 状态与查询历史；由页面层连同回调一起传下来。 */
export interface GraphQlPanelTools {
  schema: GraphQlSchema | null;
  schemaStatus: GraphQlSchemaStatus;
  schemaError: string | null;
  onFetchSchema: () => void;
  history: QueryHistoryEntry[];
  onPickHistory: (entry: QueryHistoryEntry) => void;
}

/** 历史列表里一行显示查询的第一行 —— 整段查询塞进去会把列表撑成一堵墙。 */
const HISTORY_LINE_LIMIT = 60;

function firstLine(query: string): string {
  const line =
    query
      .split('\n')
      .find((candidate) => candidate.trim().length > 0)
      ?.trim() ?? '';
  return line.length > HISTORY_LINE_LIMIT ? `${line.slice(0, HISTORY_LINE_LIMIT)}…` : line;
}

const FETCH_LABEL: Record<GraphQlSchemaStatus, string> = {
  idle: '拉取 schema',
  loading: '拉取中…',
  ready: '刷新 schema',
  error: '重试',
};

/**
 * GraphQL 页签：schema 状态、查询历史、查询、Variables、Operation name。
 *
 * 各段都写进草稿的 `graphql`，与 Params / Headers / Auth 一样由父组件的保存路径落盘。切换
 * 到 GraphQL 不清空 body：切回 HTTP 时那份 json / 表单还在，用户不该为看一眼查询付出重填的
 * 代价。
 *
 * Variables 的解析问题在这里就地显示：它不阻塞渲染，也不等发送才报 —— 用户敲完一个逗号
 * 就能看见自己的 JSON 坏在哪。发送前的最后一道门是 `validateRequest`。
 *
 * schema 与历史都是可选的加成：没拉 schema 时照常编辑与发送，只是没有补全候选。
 */
export function GraphQlPanel({
  value,
  onChange,
  schema,
  schemaStatus,
  schemaError,
  onFetchSchema,
  history,
  onPickHistory,
}: { value: GraphQlRequest | null; onChange: (next: GraphQlRequest) => void } & GraphQlPanelTools) {
  const graphql: GraphQlRequest = value ?? { query: '', variables: '', operationName: '' };
  const variablesError = variablesProblem(graphql.variables ?? '');
  const [historyOpen, setHistoryOpen] = useState(false);
  const historyRef = useRef<HTMLDivElement>(null);

  // 下拉开着时点外面或按 Esc 就收起：列表浮在编辑器上，不收掉会挡住 Variables。
  useEffect(() => {
    if (!historyOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (historyRef.current?.contains(event.target as Node)) return;
      setHistoryOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setHistoryOpen(false);
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [historyOpen]);

  // 补全候选按光标处的文档现算：schema 换了就换一份来源，查询文本不参与缓存。
  const completion = useMemo<GraphQlCompletionSource | undefined>(() => {
    if (schema === null) return undefined;
    return (context) => {
      const document = context.state.doc.toString();
      const candidates = completionCandidates(document, context.pos, schema);
      if (candidates.length === 0) return null;
      let from = context.pos;
      while (from > 0 && /\w/.test(document[from - 1] ?? '')) from -= 1;
      return {
        from,
        options: candidates.map((candidate) => ({
          label: candidate.label,
          detail: candidate.detail,
          type: 'property',
        })),
      };
    };
  }, [schema]);

  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          className="h-7 px-2"
          onClick={onFetchSchema}
          disabled={schemaStatus === 'loading'}
          data-testid="graphql-fetch-schema"
        >
          {FETCH_LABEL[schemaStatus]}
        </Button>
        {schemaStatus === 'ready' && schema !== null ? (
          <span className="text-xs text-muted-foreground" data-testid="graphql-schema-status">
            已载入 {Object.keys(schema.types).length} 个类型
          </span>
        ) : schemaStatus === 'error' ? (
          <span className="text-xs text-destructive" data-testid="graphql-schema-status">
            {schemaError}
          </span>
        ) : (
          <span className="text-xs text-muted-foreground" data-testid="graphql-schema-status">
            {schemaStatus === 'loading' ? '拉取中…' : '没有 schema：字段名按记忆输入'}
          </span>
        )}
        <div className="relative ml-auto" ref={historyRef}>
          <Button
            variant="outline"
            size="sm"
            className="h-7 px-2"
            onClick={() => setHistoryOpen((open) => !open)}
            disabled={history.length === 0}
            data-testid="graphql-history"
          >
            {history.length === 0 ? '还没有查询历史' : `查询历史（${history.length}）`}
          </Button>
          {historyOpen && (
            <div className="absolute right-0 z-10 mt-1 max-h-64 w-96 overflow-y-auto rounded-md border border-border bg-card shadow-md">
              {history.map((entry, index) => (
                <button
                  key={`${index}-${entry.query}`}
                  type="button"
                  className="block w-full cursor-pointer border-b border-border/60 px-3 py-1.5 text-left font-mono text-xs last:border-b-0 hover:bg-overlay-hover"
                  title={entry.query}
                  onClick={() => {
                    onPickHistory(entry);
                    setHistoryOpen(false);
                  }}
                  data-testid="graphql-history-item"
                >
                  {firstLine(entry.query)}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
      <section className="flex flex-col gap-1.5">
        <span className="text-xs font-medium text-muted-foreground">查询</span>
        <GraphQlEditor
          value={graphql.query}
          onChange={(query) => onChange({ ...graphql, query })}
          language="graphql"
          height="240px"
          placeholder={'query User($id: ID!) {\n  user(id: $id) { id name }\n}'}
          completion={completion}
        />
      </section>
      <section className="flex flex-col gap-1.5">
        <span className="text-xs font-medium text-muted-foreground">Variables（JSON 对象）</span>
        <GraphQlEditor
          value={graphql.variables ?? ''}
          onChange={(variables) => onChange({ ...graphql, variables })}
          language="json"
          height="140px"
          placeholder={'{\n  "id": "1"\n}'}
        />
        {variablesError !== null && (
          <p className="text-xs text-destructive" data-testid="graphql-variables-error">
            {variablesError}
          </p>
        )}
      </section>
      <section className="flex w-72 flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground" htmlFor="graphql-operation">
          Operation name
        </label>
        <Input
          id="graphql-operation"
          value={graphql.operationName ?? ''}
          onChange={(e) => onChange({ ...graphql, operationName: e.target.value })}
          placeholder="可选：多操作的文档里指定执行哪一个"
          className="h-9 font-mono text-sm"
        />
      </section>
      <p className="text-xs text-muted-foreground">
        GraphQL 走 HTTP POST：URL、Headers、Params、Auth 与变量替换和 HTTP 请求同一套规则。
      </p>
    </div>
  );
}
