/**
 * Real HTTP sender — single transport, two distribution shapes:
 *
 *   1. Tauri shell  → `invokeCmd(COMMANDS.httpRequest, …)` hits the
 *                     `http_request` Rust command, which delegates to
 *                     `http_executor::execute` (reqwest). Bypasses the
 *                     webview's CORS enforcement entirely.
 *   2. Web / Dev    → `invokeCmd` falls through to `webInvokeCmd` →
 *                     `POST /api/http/send` on the axum `serve-web`
 *                     host, which calls the same `http_executor::execute`.
 *                     Same Rust path, same response shape.
 *
 * Both shapes return the same `RustHttpResult` envelope; the rest of
 * the app (HttpTester, ResponseViewer, the cURL exporter) doesn't
 * care which one ran.
 *
 * Contract:
 *   - Builds URL + init via `buildFetchInit` (single source of truth
 *     shared with the cURL exporter). Only the URL, headers (as a
 *     tuple array) and body *string* are sent — the rest is a
 *     JSON-friendly payload.
 *   - Composes a per-call AbortController from the caller's `signal`
 *     and arms `settings.timeoutMs`. The signal is forwarded to
 *     `invokeCmd` which passes it to native `fetch` in the web
 *     shape; the Tauri shape rejects the JS promise but lets the
 *     Rust `reqwest` keep running to completion.
 *   - Returns an `ApiResponse`. Network failures, timeouts, and aborts
 *     surface as `status: 0` with distinguishing `statusText` so the
 *     UI can render the right banner.
 *   - Never throws — `HttpTester` just dispatches the returned
 *     `ApiResponse` into the store.
 */
import type {
  ApiResponse,
  AuthConfig,
  Environment,
  HttpMethod,
  KeyValue,
  RequestBody,
} from '@/types/domain';
import type { RequestSettings } from '@/stores/app-store';
import { COMMANDS, invokeCmd, type CallApiOptions } from './tauri-commands';
import { buildFetchInit } from './http-build';

export interface SendInput {
  method: HttpMethod;
  url: string;
  params: KeyValue[];
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
  env?: Environment;
  settings: RequestSettings;
  /** Optional abort signal from the caller (component unmount,
   *  re-click on Send to cancel the previous in-flight call). */
  signal?: AbortSignal;
}

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

/** Same flattening but starting from `Array<[name, value]>` (Rust
 *  serialises headers as a tuple list across both shapes). */
function tuplesToRecord(pairs: ReadonlyArray<readonly [string, string]>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of pairs) {
    if (out[k] === undefined) out[k] = v;
    else out[k] = `${out[k]}, ${v}`;
  }
  return out;
}

/** Best-effort JSON parse — leave `bodyJson` undefined on failure
 *  (per spec, no throw). */
function tryParseJson(text: string): unknown | undefined {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

// ---------------------------------------------------------------------------
// error shape builders — shared so the UI sees the same banner regardless
// of which underlying transport (Tauri IPC vs. serve-web HTTP) ran.
// ---------------------------------------------------------------------------

function timeoutResponse(input: SendInput, url: string, durationMs: number): ApiResponse {
  return {
    status: 0,
    statusText: 'Timeout',
    error: `Request timeout after ${input.settings.timeoutMs}ms`,
    durationMs,
    sizeBytes: 0,
    headers: {},
    body: '',
    contentType: '',
    method: input.method,
    url,
  };
}

function abortedResponse(input: SendInput, url: string, durationMs: number): ApiResponse {
  return {
    status: 0,
    statusText: 'Aborted',
    error: 'aborted',
    durationMs,
    sizeBytes: 0,
    headers: {},
    body: '',
    contentType: '',
    method: input.method,
    url,
  };
}

function networkErrorResponse(
  input: SendInput,
  url: string,
  durationMs: number,
  message: string,
): ApiResponse {
  return {
    status: 0,
    statusText: 'Network Error',
    error: message,
    durationMs,
    sizeBytes: 0,
    headers: {},
    body: '',
    contentType: '',
    method: input.method,
    url,
  };
}

// ---------------------------------------------------------------------------
// Backend result envelope — matches `crate::http_executor::HttpResult`
// (camelCase fields, see `#[serde(rename_all = "camelCase")]` on the
// Rust struct). Both the Tauri command and the axum `/api/http/send`
// route return this exact shape so we can deserialise with one type.
// ---------------------------------------------------------------------------

interface RustHttpOk {
  status: number;
  statusText: string;
  durationMs: number;
  sizeBytes: number;
  headers: Array<[string, string]>;
  body: string;
  contentType: string;
  method: string;
  url: string;
  truncated: boolean;
}
interface RustHttpErr {
  kind: 'timeout' | 'network' | string;
  message: string;
}
interface RustHttpResult {
  ok: boolean;
  response?: RustHttpOk;
  error?: RustHttpErr;
}

/** Convert any `HeadersInit` shape into a list of `[name, value]`
 *  tuples. Mirrors the helper the old `sendViaTauri` carried. */
function initHeadersToTuples(h: HeadersInit): Array<[string, string]> {
  if (Array.isArray(h)) return h as Array<[string, string]>;
  if (typeof Headers !== 'undefined' && h instanceof Headers) {
    const out: Array<[string, string]> = [];
    h.forEach((v, k) => out.push([k, v]));
    return out;
  }
  if (h && typeof h === 'object') return Object.entries(h as Record<string, string>);
  return [];
}

// ---------------------------------------------------------------------------
// public entry — single transport, one call site.
// ---------------------------------------------------------------------------

/**
 * Convert the backend `HttpResult` envelope into the renderer's
 * `ApiResponse`. Shared by the raw-URL path and the endpoint-scoped path
 * so both surfaces produce byte-identical UI state.
 */
function envelopeToApiResponse(r: RustHttpResult, input: SendInput, url: string): ApiResponse {
  if (!r.ok || !r.response) {
    // Round-trip succeeded; the server-side request itself failed.
    const kind = r.error?.kind ?? 'network';
    const msg = r.error?.message ?? 'unknown';
    if (kind === 'timeout') {
      return timeoutResponse(input, url, 0);
    }
    return networkErrorResponse(input, url, 0, msg);
  }

  const resp = r.response;
  return {
    status: resp.status,
    statusText: resp.statusText || (resp.status >= 200 && resp.status < 300 ? 'OK' : 'Error'),
    durationMs: resp.durationMs,
    sizeBytes: resp.sizeBytes,
    headers: tuplesToRecord(resp.headers),
    body: resp.body,
    bodyJson: tryParseJson(resp.body),
    contentType: resp.contentType,
    method: input.method,
    url,
  };
}

/** Dispatch an `invokeCmd` call and fold the result into an
 *  `ApiResponse`, never throwing. */
async function invokeAndMap(
  args: Record<string, unknown>,
  input: SendInput,
  url: string,
  options: CallApiOptions,
): Promise<ApiResponse> {
  try {
    const r = await invokeCmd<RustHttpResult>(COMMANDS.httpRequest, args, options);
    return envelopeToApiResponse(r, input, url);
  } catch (err) {
    // `invokeCmd` rejects on:
    //   - Tauri IPC failure (e.g. devtools not wired)
    //   - serve-web HTTP non-2xx / network unreachable
    //   - WEB_ROUTES missing the command (would only happen if
    //     someone added a COMMANDS entry without a matching route)
    //   - the caller's AbortSignal aborting (Tauri shape rejects,
    //     web shape throws DOMException)
    // We surface abort distinctly; everything else falls back to
    // the generic Network Error banner so the UI stays informative.
    const name = (err as { name?: string })?.name;
    const msg = (err as Error)?.message ?? String(err);
    if (name === 'AbortError' || /abort/i.test(msg)) {
      return abortedResponse(input, url, 0);
    }
    console.error('[http.ts] backend invoke failed:', err);
    return networkErrorResponse(input, url, 0, msg);
  }
}

/**
 * Send the request described by `input` via the backend transport
 * (`invokeCmd(COMMANDS.httpRequest, …)`). See module header.
 *
 * The transport is picked once at module load:
 *   - Tauri shell → `invoke()` IPC → Rust `http_executor::execute`
 *   - serve-web   → `POST /api/http/send` → same Rust executor
 *
 * Both shapes hit the same `reqwest`-based executor so we get
 * identical behaviour: timeout, redirect, max body, body truncation
 * all happen server-side and the result crosses the boundary as a
 * JSON envelope.
 *
 * ⚠️ This path lets the caller name an arbitrary URL, so the server
 * becomes a proxy. It stays for the explicit "raw URL" case; the UI's
 * Send button uses [`sendEndpoint`] instead, which cannot.
 *
 * Note: there is no "raw browser fetch" fallback. In every supported
 * distribution shape there's a real backend in front of `reqwest`,
 * so a direct browser fetch would only ever be relevant for a
 * hypothetical Vite-only / no-server mode that we don't ship.
 */
export async function sendRequest(input: SendInput): Promise<ApiResponse> {
  const built = buildFetchInit(input);
  // `buildFetchInit` returns a `RequestInit` with `Headers | [][]
  // | Record`. Normalise to a tuple list — Rust deserialises
  // `Vec<(String, String)>` and the web bridge JSON-encodes a tuple
  // list verbatim, so this is the only shape that works on both sides.
  const headers = initHeadersToTuples(built.init.headers ?? {});
  const body =
    typeof built.init.body === 'string' && built.init.body.length > 0 ? built.init.body : null;
  const options: CallApiOptions = input.signal ? { signal: input.signal } : {};

  return invokeAndMap(
    {
      method: input.method,
      url: built.url,
      headers,
      body,
      timeoutMs: input.settings.timeoutMs,
      followRedirects: input.settings.followRedirects,
      maxResponseSizeKb: input.settings.maxResponseSizeKb,
    },
    input,
    built.url,
    options,
  );
}

// ---------------------------------------------------------------------------
// Endpoint-scoped send — what the UI's Send button uses.
// ---------------------------------------------------------------------------

export interface SendEndpointInput {
  /** Stored endpoint to send. The backend reads method, params,
   *  headers, body, auth and the base-URL definition from the database. */
  endpointId: string;
  /** Draft URL **path** (no base URL) from the URL input, or omitted to
   *  use the stored one. The input box only ever holds the path, so this
   *  is what the user sees; the backend joins it onto the base URL. */
  urlOverride?: string;
  /** The environment whose variables and base URLs the user is looking
   *  at right now. Required in practice: the ENV pill is Zustand-only, so
   *  the database's `is_active` does not follow the selection. */
  environmentId?: string;
  method: HttpMethod;
  settings: RequestSettings;
  signal?: AbortSignal;
}

/**
 * Send a stored endpoint by id.
 *
 * The renderer never supplies a finished URL, so this route has no SSRF
 * surface: the target is derived from rows the user configured. URL
 * composition — base URL precedence, `{{var}}` substitution, auth
 * reduction — happens once, in Rust, by
 * `simple_post_mcp::variable_fill::resolve_endpoint`.
 *
 * `url` in the returned `ApiResponse` is filled from the response
 * envelope, which carries the URL the backend actually dialled.
 */
export async function sendEndpoint(input: SendEndpointInput): Promise<ApiResponse> {
  // The envelope mapper wants a `SendInput` only for the method and the
  // synthetic URL shown on a failure; the real URL arrives in the result.
  const shape: SendInput = {
    method: input.method,
    url: '',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    settings: input.settings,
  };
  const options: CallApiOptions = input.signal ? { signal: input.signal } : {};

  try {
    const r = await invokeCmd<RustHttpResult>(
      COMMANDS.endpointSend,
      {
        endpointId: input.endpointId,
        overrides: {
          // All three travel. `url` and `method` are the renderer's
          // unsaved draft; `environmentId` is the ENV pill's selection.
          // None is redundant: the method picker and URL box write to the
          // store draft rather than the endpoint row, and the ENV pill is
          // Zustand-only, so omitting any of them makes the wire disagree
          // with what the user sees.
          url: input.urlOverride,
          method: input.method,
          environmentId: input.environmentId,
        },
        settings: {
          timeoutMs: input.settings.timeoutMs,
          followRedirects: input.settings.followRedirects,
          maxResponseSizeKb: input.settings.maxResponseSizeKb,
        },
      },
      options,
    );
    // `shape.method` is the draft method, which is now also what travels in
    // `overrides.method`, so the badge cannot disagree with the request.
    return envelopeToApiResponse(r, shape, r.response?.url ?? '');
  } catch (err) {
    const name = (err as { name?: string })?.name;
    const msg = (err as Error)?.message ?? String(err);
    if (name === 'AbortError' || /abort/i.test(msg)) {
      return abortedResponse(shape, '', 0);
    }
    console.error('[http.ts] endpoint_send failed:', err);
    return networkErrorResponse(shape, '', 0, msg);
  }
}
