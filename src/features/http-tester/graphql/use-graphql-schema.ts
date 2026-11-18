import { useCallback, useEffect, useRef, useState } from 'react';

import { sendPreparedRequest } from '@/lib/http';
import { INTROSPECTION_QUERY, parseIntrospection, type GraphQlSchema } from '@/lib/graphql-schema';
import type { AuthConfig } from '@/types/domain';
import type { RequestSettings } from '@/stores/store-types';
import type { PreparedRequest } from '@/lib/http-build/types';

export type GraphQlSchemaStatus = 'idle' | 'loading' | 'ready' | 'error';

export interface GraphQlSchemaState {
  status: GraphQlSchemaStatus;
  schema: GraphQlSchema | null;
  /** 拉取失败的原因，直接给用户看；成功与进行中为 null。 */
  error: string | null;
}

export interface GraphQlSchemaController extends GraphQlSchemaState {
  refresh: () => void;
}

/**
 * 会话内缓存。键带 URL：同一个端点改了地址，schema 就属于另一个服务，不能复用。
 */
const schemaCache = new Map<string, GraphQlSchema>();

const IDLE: GraphQlSchemaState = { status: 'idle', schema: null, error: null };

function cacheKey(endpointId: string, url: string): string {
  return `${endpointId}|${url}`;
}

function stateFor(key: string): GraphQlSchemaState {
  const cached = schemaCache.get(key);
  return cached ? { status: 'ready', schema: cached, error: null } : IDLE;
}

/**
 * 把 `Content-Type` 钉成 `application/json`：用户自己写过就地替换值，没写过追加一条。
 *
 * 重名的 Content-Type 只留第一条 —— 两条同名 header 里有一条写的是别的类型，服务端读哪条
 * 都不由我们决定。
 */
function withJsonContentType(
  headers: ReadonlyArray<readonly [string, string]>,
): Array<[string, string]> {
  let replaced = false;
  const out: Array<[string, string]> = [];
  for (const [name, value] of headers) {
    if (name.toLowerCase() !== 'content-type') {
      out.push([name, value]);
      continue;
    }
    if (replaced) continue;
    replaced = true;
    out.push([name, 'application/json']);
  }
  if (!replaced) out.push(['Content-Type', 'application/json']);
  return out;
}

interface Args {
  endpointId: string;
  /** 这次要发的那份已组装请求：URL 与 headers 直接复用，不另拼一份。 */
  request: PreparedRequest;
  auth: AuthConfig;
  settings: RequestSettings;
}

/**
 * 为当前端点拉 GraphQL schema 并缓存。按需触发（面板上的按钮），不随渲染自动请求：
 * introspection 也是一次真实请求，用户没点就不该发出去。
 */
export function useGraphQlSchema({
  endpointId,
  request,
  auth,
  settings,
}: Args): GraphQlSchemaController {
  const key = cacheKey(endpointId, request.url);
  // 异步结果回来时端点或 URL 可能已经变了：只认发起那一刻的键。
  const keyRef = useRef(key);
  keyRef.current = key;

  const [state, setState] = useState<GraphQlSchemaState>(() => stateFor(key));

  useEffect(() => {
    setState(stateFor(key));
  }, [key]);

  const refresh = useCallback(() => {
    const startedFor = key;
    const payload = JSON.stringify({ query: INTROSPECTION_QUERY });
    setState({ status: 'loading', schema: null, error: null });

    void sendPreparedRequest({
      request: {
        ...request,
        method: 'POST',
        headers: withJsonContentType(request.headers),
        body: payload,
        bodyBase64: null,
      },
      auth,
      settings,
    })
      .then(({ response }) => {
        if (keyRef.current !== startedFor) return;
        if (response.status < 200 || response.status >= 300) {
          setState({
            status: 'error',
            schema: null,
            error: `拉取 schema 失败：HTTP ${response.status}`,
          });
          return;
        }
        let parsed: unknown;
        try {
          parsed = JSON.parse(response.body);
        } catch {
          parsed = null;
        }
        const schema = parseIntrospection(parsed);
        if (!schema) {
          setState({
            status: 'error',
            schema: null,
            error: '这个响应不是 GraphQL introspection 结果（服务端可能关掉了 introspection）',
          });
          return;
        }
        schemaCache.set(startedFor, schema);
        if (keyRef.current === startedFor) {
          setState({ status: 'ready', schema, error: null });
        }
      })
      .catch((err: unknown) => {
        if (keyRef.current !== startedFor) return;
        setState({
          status: 'error',
          schema: null,
          error: `拉取 schema 失败：${(err as Error)?.message ?? String(err)}`,
        });
      });
  }, [key, request, auth, settings]);

  return { ...state, refresh };
}
