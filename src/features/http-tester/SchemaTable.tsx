/**
 * Schema 的只读属性表 —— 把一份 JSON Schema 画成「字段名 / 类型 / 必填 / 说明 / 枚举与默认值」五列。
 *
 * 为什么是只读的
 * --------------
 * 这一块只回答「这份定义长什么样」，不负责改它。改它的地方是同一处的「JSON」视图：
 * 两个视图共用同一份文本（`RequestBody.schema`），故它们不可能各说各话。
 *
 * 为什么不用 CodeMirror
 * --------------------
 * CodeMirror 是编辑器，而这一块没有可编辑的内容。body 页签里已经挂了一份 CodeMirror，
 * 多挂一份编辑器只会多下载一次编辑器代码，换不来任何东西。
 *
 * 表格的取值全部来自 `lib/schema-view` 的解析结果：这个组件不自己读 JSON，
 * 故「界面显示的字段」与「解析规则」永远是同一套结论。
 */
import { Fragment } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { cn, focusRing } from '@/lib/utils';
import type { SchemaField, SchemaViewResult } from '@/lib/schema-view';
import { useLocalExpansion } from '@/hooks/useLocalExpansion';

/** 字段在表格里的缩进宽度（像素）。 */
const INDENT = 14;

export function SchemaTable({ result }: { result: SchemaViewResult }) {
  const { isExpanded, toggle } = useLocalExpansion();

  if (!result.ok) {
    return (
      <div className="rounded-md border border-destructive/40 px-3 py-3 text-xs">
        <p className="font-medium text-destructive">
          {result.kind === 'parse-error' ? '这份定义解析不了' : '这份定义不是一个对象'}
        </p>
        <p className="mt-1 break-words text-muted-foreground">{result.message}</p>
        <p className="mt-1 text-muted-foreground">切到「JSON」视图修正这段文本。</p>
      </div>
    );
  }

  const { view } = result;

  if (view.empty) {
    return (
      <div className="rounded-md border border-dashed border-border px-3 py-4 text-center text-xs text-muted-foreground">
        还没有写 Schema 定义。切到「JSON」视图填写这份接口接受的参数结构。
      </div>
    );
  }

  if (!view.fields.length) {
    return (
      <div className="rounded-md border border-dashed border-border px-3 py-3 text-xs text-muted-foreground">
        <p>这份定义没有列出字段。</p>
        {view.type ? <p className="mt-1">根节点类型：{view.type}</p> : null}
        {view.description ? <p className="mt-1">说明：{view.description}</p> : null}
        {view.note ? <p className="mt-1">{view.note}</p> : null}
      </div>
    );
  }

  return (
    <div className="overflow-hidden rounded-md border border-border">
      <table className="w-full table-fixed text-xs">
        <colgroup>
          <col className="w-[26%]" />
          <col className="w-[17%]" />
          <col className="w-[9%]" />
          {/* 说明列吃掉剩余宽度：字段名、类型与取值的长度差别很大，说明最能展开。 */}
          <col />
          <col className="w-[24%]" />
        </colgroup>
        <thead>
          <tr className="border-b border-border bg-muted/40 text-[11px] text-muted-foreground">
            <th scope="col" className="px-2 py-1.5 text-left font-medium">
              字段名
            </th>
            <th scope="col" className="px-2 py-1.5 text-left font-medium">
              类型
            </th>
            <th scope="col" className="px-2 py-1.5 text-left font-medium">
              必填
            </th>
            <th scope="col" className="px-2 py-1.5 text-left font-medium">
              说明
            </th>
            <th scope="col" className="px-2 py-1.5 text-left font-medium">
              枚举 / 默认值
            </th>
          </tr>
        </thead>
        <tbody>
          <SchemaRows
            fields={view.fields}
            depth={0}
            pathPrefix=""
            isExpanded={isExpanded}
            onToggle={toggle}
          />
        </tbody>
      </table>
    </div>
  );
}

interface RowsProps {
  fields: SchemaField[];
  /** 当前这一层在表格里的缩进层数。 */
  depth: number;
  /** 当前这一层的路径前缀，用来给展开状态一个唯一键。 */
  pathPrefix: string;
  /** 这一行的路径是不是展开着。 */
  isExpanded: (path: string) => boolean;
  onToggle: (path: string) => void;
}

/** 一层字段行。嵌套字段展开时，递归渲染下一层。 */
function SchemaRows({ fields, depth, pathPrefix, isExpanded, onToggle }: RowsProps) {
  return (
    <>
      {fields.map((field) => {
        const path = pathPrefix ? `${pathPrefix}/${field.name}` : field.name;
        const children = field.children ?? [];
        const hasChildren = children.length > 0;
        const open = isExpanded(path);

        return (
          <Fragment key={path}>
            <tr className="border-b border-border/50 align-top last:border-b-0">
              <td className="px-2 py-1.5">
                <div className="flex items-start gap-1" style={{ paddingLeft: depth * INDENT }}>
                  {hasChildren ? (
                    <button
                      type="button"
                      onClick={() => onToggle(path)}
                      aria-expanded={open}
                      aria-label={open ? `收起 ${field.name}` : `展开 ${field.name}`}
                      title={open ? '收起' : '展开'}
                      className={cn(
                        'mt-px shrink-0 rounded p-0.5 text-muted-foreground transition-colors',
                        'hover:bg-overlay-hover hover:text-foreground',
                        focusRing,
                      )}
                    >
                      {open ? (
                        <ChevronDown className="h-3 w-3" aria-hidden />
                      ) : (
                        <ChevronRight className="h-3 w-3" aria-hidden />
                      )}
                    </button>
                  ) : (
                    <span className="mt-px h-3.5 w-3.5 shrink-0" aria-hidden />
                  )}
                  <span className="font-mono text-[11px] break-all">{field.name}</span>
                </div>
              </td>
              <td className="px-2 py-1.5">
                <span className="font-mono text-[11px] break-all">{field.type}</span>
                {field.format ? (
                  <span className="ml-1 text-[10px] text-muted-foreground">{field.format}</span>
                ) : null}
              </td>
              <td className="px-2 py-1.5">
                {/* 必填与否同时用文字写出来：只靠颜色区分，浅色主题下读不出来。 */}
                {field.required ? (
                  <span className="inline-block rounded bg-muted px-1 py-0.5 text-[10px] font-semibold text-foreground">
                    必填
                  </span>
                ) : (
                  <span className="text-[10px] text-muted-foreground">可选</span>
                )}
              </td>
              <td className="px-2 py-1.5">
                {field.description ? (
                  <p className="break-words text-muted-foreground">{field.description}</p>
                ) : null}
                {field.note ? (
                  <p className="mt-0.5 break-words text-[10px] text-muted-foreground">
                    {field.note}
                  </p>
                ) : null}
                {!field.description && !field.note ? (
                  <span className="text-muted-foreground">—</span>
                ) : null}
              </td>
              <td className="px-2 py-1.5">
                {field.enumValues?.length ? (
                  <div className="flex flex-wrap gap-1">
                    {field.enumValues.map((value, index) => (
                      <span
                        key={`${value}-${index}`}
                        className="rounded bg-muted px-1 py-0.5 font-mono text-[10px]"
                      >
                        {value}
                      </span>
                    ))}
                  </div>
                ) : null}
                {field.defaultValue !== undefined ? (
                  <p className="mt-1 text-[10px] text-muted-foreground">
                    默认 <span className="font-mono">{field.defaultValue}</span>
                  </p>
                ) : null}
                {!field.enumValues?.length && field.defaultValue === undefined ? (
                  <span className="text-muted-foreground">—</span>
                ) : null}
              </td>
            </tr>
            {hasChildren && open ? (
              <SchemaRows
                fields={children}
                depth={depth + 1}
                pathPrefix={path}
                isExpanded={isExpanded}
                onToggle={onToggle}
              />
            ) : null}
          </Fragment>
        );
      })}
    </>
  );
}
