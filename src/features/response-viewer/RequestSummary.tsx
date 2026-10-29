import { useMemo } from 'react';
import { Globe } from 'lucide-react';
import type { ApiResponse, StoredRequestSnapshot } from '@/types/domain';
import { cn, formatBytes, formatDuration, methodColorVar, tryFormatJson } from '@/lib/utils';

/**
 * 请求详情 —— 响应区左栏那份「这一次实际发出去的请求」。
 *
 * 画的是**线上那一份**：base URL 与路径已拼接、路径参数已替换、`{{var}}` 已换成取到的
 * 值、查询串已追加、header 是后端在请求发出后报上来的那一份（含传输层补的 `accept` /
 * `accept-encoding` / `host` / `content-length`）。文档里那份模板随时可以再打开看，
 * 线上真正发出去的那一份只有这一栏能看到。
 *
 * 分段方式照浏览器开发者工具的请求详情：常规信息、查询字符串参数、请求头、请求体。
 * 那一栏要回答的是「服务端刚才收到的是什么」，与右栏那份「服务端刚才回了什么」
 * 配成一对。
 */
export function RequestSummary({
  request,
  response,
}: {
  request: StoredRequestSnapshot | null;
  /** 那次响应。缺席时（还没发过）只有请求那一半的信息。 */
  response?: ApiResponse | null;
}) {
  if (!request) {
    return (
      <div className="flex h-full items-center justify-center px-6 py-8 text-center">
        <div>
          <div className="mx-auto flex h-10 w-10 items-center justify-center rounded-full bg-muted/40">
            <Globe className="h-5 w-5 text-muted-foreground/50" />
          </div>
          <p className="mt-3 text-xs text-muted-foreground">这次调用没有请求记录</p>
          <p className="mt-1 text-[11px] text-muted-foreground/70">
            发送一次请求后，这里会显示它实际发出去的地址、请求头与请求体
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4 px-3 py-3">
      <div className="flex items-center gap-1.5">
        <span className="label-caps text-muted-foreground/70">请求详情</span>
        <span className="text-[10px] text-muted-foreground/70">这一次实际发出去的内容</span>
      </div>

      <GeneralSection request={request} response={response ?? null} />
      <QuerySection url={request.url} />
      <HeadersSection headers={request.headers} />
      <BodySection body={request.body} contentType={contentTypeOf(request.headers)} />
    </div>
  );
}

/** 从请求头里找出 `content-type`。请求体按它决定怎么展示。 */
function contentTypeOf(headers: Array<[string, string]>): string {
  const hit = headers.find(([name]) => name.toLowerCase() === 'content-type');
  return hit ? hit[1] : '';
}

/**
 * 常规信息。
 *
 * 与开发者工具的那一段同名同义：请求方法、请求 URL、状态码、耗时，以及两侧的体积。
 */
function GeneralSection({
  request,
  response,
}: {
  request: StoredRequestSnapshot;
  response: ApiResponse | null;
}) {
  const bodyBytes = request.body ? new TextEncoder().encode(request.body).length : 0;
  return (
    <Section title="常规">
      <div className="flex flex-col gap-0.5">
        <Pair label="请求方法">
          <span
            className="inline-flex h-5 shrink-0 items-center justify-center rounded px-1.5 font-mono text-[10px] font-semibold text-white"
            style={{ backgroundColor: methodColorVar(request.method) }}
          >
            {request.method}
          </span>
        </Pair>
        <Pair label="请求 URL">
          <span className="font-mono text-[11px] leading-relaxed text-foreground/90 break-all">
            {request.url}
          </span>
        </Pair>
        {response && (
          <>
            <Pair label="状态码">
              <span className="font-mono text-[11px] text-foreground/90">
                {response.status} {response.statusText}
              </span>
            </Pair>
            <Pair label="耗时">
              <span className="font-mono text-[11px] text-foreground/90">
                {formatDuration(response.durationMs)}
              </span>
            </Pair>
            <Pair label="响应大小">
              <span className="font-mono text-[11px] text-foreground/90">
                {formatBytes(response.sizeBytes)}
              </span>
            </Pair>
          </>
        )}
        {request.body !== null && (
          <Pair label="请求体大小">
            <span className="font-mono text-[11px] text-foreground/90">
              {formatBytes(bodyBytes)}
            </span>
          </Pair>
        )}
      </div>
    </Section>
  );
}

/**
 * 查询字符串参数。
 *
 * 参数已经并进最终 URL 了，故这里从 URL 上拆出来单列一份：URL 里那一长串问号后面的
 * 东西，逐个参数读比读整条地址容易。
 */
function QuerySection({ url }: { url: string }) {
  const query = useMemo(() => {
    const idx = url.indexOf('?');
    if (idx < 0) return [] as Array<[string, string]>;
    return Array.from(new URLSearchParams(url.slice(idx + 1)).entries());
  }, [url]);

  if (query.length === 0) return null;

  return (
    <Section title="查询字符串参数">
      <Table
        rows={query.map(([key, value]) => ({ name: key, value }))}
        nameHeader="参数名"
        valueHeader="参数值"
      />
    </Section>
  );
}

/** 请求头。线上那一份，故含传输层补的那几条。 */
function HeadersSection({ headers }: { headers: Array<[string, string]> }) {
  if (headers.length === 0) return null;
  return (
    <Section title="请求头">
      <Table
        rows={headers.map(([name, value]) => ({ name, value }))}
        nameHeader="名称"
        valueHeader="值"
      />
    </Section>
  );
}

/**
 * 请求体。
 *
 * JSON 体格式化后读：调试时要看的通常是字段与取值，不是缩进。表单编码的体拆成
 * 表格，与开发者工具展示表单字段的方式一致。
 */
function BodySection({ body, contentType }: { body: string | null; contentType: string }) {
  const isForm = contentType.toLowerCase().includes('application/x-www-form-urlencoded');

  const formRows = useMemo(() => {
    if (!isForm || !body) return [] as Array<{ name: string; value: string }>;
    return Array.from(new URLSearchParams(body).entries()).map(([name, value]) => ({
      name,
      value,
    }));
  }, [isForm, body]);

  const text = useMemo(() => {
    if (body === null || body === '') return null;
    const parsed = tryFormatJson(body);
    return parsed.ok ? parsed.formatted : body;
  }, [body]);

  if (text === null) return null;

  if (formRows.length > 0) {
    return (
      <Section title="请求体">
        <Table rows={formRows} nameHeader="字段名" valueHeader="字段值" />
      </Section>
    );
  }

  return (
    <Section title="请求体">
      <pre className="m-0 max-h-72 overflow-auto whitespace-pre-wrap break-all rounded bg-muted/30 p-2 font-mono text-[11px] leading-relaxed text-foreground/85">
        {text}
      </pre>
    </Section>
  );
}

/** 一段。 */
function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <div className="label-caps mb-1 text-muted-foreground/70">{title}</div>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

/** 常规信息里的一行：左边是名字，右边是值。 */
function Pair({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex min-w-0 items-baseline gap-2 text-[11px]">
      <span className="w-20 shrink-0 text-muted-foreground/70">{label}</span>
      <span className="min-w-0 flex-1">{children}</span>
    </div>
  );
}

/** 两列的表：左边是名字，右边是值。 */
function Table({
  rows,
  nameHeader,
  valueHeader,
}: {
  rows: Array<{ name: string; value: string }>;
  nameHeader: string;
  valueHeader: string;
}) {
  return (
    <div className="overflow-hidden rounded border border-border/60">
      <table className="w-full table-fixed border-collapse text-[11px]">
        <thead>
          <tr className="bg-muted/30 text-left text-muted-foreground/70">
            <th className="w-1/3 truncate border-b border-border/60 px-2 py-1 font-normal">
              {nameHeader}
            </th>
            <th className="truncate border-b border-border/60 px-2 py-1 font-normal">
              {valueHeader}
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row, idx) => (
            <tr key={`${row.name}-${idx}`} className={cn('align-top')}>
              <td className="truncate border-b border-border/40 px-2 py-1 font-mono text-primary">
                {row.name}
              </td>
              <td className="border-b border-border/40 px-2 py-1 font-mono text-foreground/80 break-all">
                {row.value}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
