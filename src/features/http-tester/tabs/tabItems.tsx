import type { ReactNode } from 'react';
import type { TabItem } from '@/components/ui/tabs';

/**
 * 请求测试器的页签 id。`path` 是路径参数，`script` 是请求脚本（前置 / 测试两段），
 * `response` 是文档用的响应定义，与发送请求无关；`graphql` 与 `body` 占同一个位置 ——
 * 协议是 GraphQL 时页签条上出现的是它，body 的内容仍留在草稿里。
 */
export type TesterTabId =
  | 'params'
  | 'path'
  | 'headers'
  | 'body'
  | 'graphql'
  | 'auth'
  | 'docs'
  | 'response'
  | 'script'
  | 'tls';

const TABS: { id: TesterTabId; label: string; disabled?: boolean }[] = [
  { id: 'params', label: 'Params' },
  { id: 'path', label: '路径参数' },
  { id: 'headers', label: 'Headers' },
  { id: 'body', label: 'Body' },
  { id: 'auth', label: 'Auth' },
  { id: 'docs', label: 'Docs' },
  { id: 'response', label: '响应' },
  { id: 'script', label: '脚本' },
  { id: 'tls', label: '证书' },
];

/** 每个页签上那枚徽标显示什么。计数为 0、开关为 false 的页签不挂徽标。 */
export interface TabBadgeState {
  params: number;
  path: number;
  headers: number;
  body: boolean;
  auth: boolean;
  /** 响应定义的条数，与 params 一样报「有几条」。 */
  response: number;
  script: boolean;
  /** 端点是否带着自己那份 HTTPS 出站设置。 */
  tls: boolean;
  /** GraphQL 查询非空 —— GraphQL 页签的圆点。 */
  graphql: boolean;
}

/** 计数徽标报「有多少项」，圆点徽标报「这一页有东西」。 */
export function buildTabItems(state: TabBadgeState, graphqlMode = false): TabItem[] {
  return TABS.map((t) => {
    // GraphQL 与 Body 互斥地占同一个位置：查询编辑器和 body 编辑器都是「请求体」这一栏。
    if (t.id === 'body' && graphqlMode) {
      return {
        id: 'graphql',
        label: 'GraphQL',
        badge: state.graphql ? (
          <span className="rounded bg-primary/20 px-1 text-nano text-primary">●</span>
        ) : null,
      };
    }
    let badge: ReactNode = null;
    if (t.id === 'params' && state.params) {
      badge = <span className="rounded bg-muted px-1 text-nano">{state.params}</span>;
    }
    if (t.id === 'path' && state.path) {
      badge = <span className="rounded bg-muted px-1 text-nano">{state.path}</span>;
    }
    if (t.id === 'headers' && state.headers) {
      badge = <span className="rounded bg-muted px-1 text-nano">{state.headers}</span>;
    }
    if (t.id === 'body' && state.body) {
      badge = <span className="rounded bg-primary/20 px-1 text-nano text-primary">●</span>;
    }
    if (t.id === 'auth' && state.auth) {
      badge = (
        <span className="rounded bg-warning/20 px-1 text-nano text-warning-foreground">●</span>
      );
    }
    if (t.id === 'response' && state.response) {
      badge = <span className="rounded bg-muted px-1 text-nano">{state.response}</span>;
    }
    if (t.id === 'script' && state.script) {
      badge = <span className="rounded bg-primary/20 px-1 text-nano text-primary">●</span>;
    }
    if (t.id === 'tls' && state.tls) {
      badge = (
        <span className="rounded bg-warning/20 px-1 text-nano text-warning-foreground">●</span>
      );
    }
    return { id: t.id, label: t.label, badge, disabled: t.disabled };
  });
}
