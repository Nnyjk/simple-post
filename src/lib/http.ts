/**
 * Real HTTP sender — dispatches between two transports:
 *
 *   1. Tauri (`invoke('http_request', …)`) when the app is running
 *      inside the desktop shell. This routes the request through a
 *      Rust `reqwest` client, which completely sidesteps the webview's
 *      CORS enforcement. This is the path that fixes the "Network
 *      Error" users saw when the app was served by plain Vite.
 *   2. Native `fetch` otherwise (browser dev mode, future web build).
 *      Kept intact so the existing single-source-of-truth builder,
 *      timeout/abort, and response-truncation logic continue to apply.
 *
 * Both transports return the same `ApiResponse` shape; the rest of the
 * app (HttpTester, ResponseViewer, the cURL exporter) doesn't care
 * which one ran.
 *
 * Contract:
 *   - Builds URL + init via `buildFetchInit` (single source of truth
 *     shared with the cURL exporter). For Tauri, only the URL,
 *     headers, and body *string* are sent — the rest is JSON-friendly
 *     payload.
 *   - Composes a per-call AbortController from the caller's `signal`
 *     and arms `settings.timeoutMs`.
 *   - Reads the response body up to `settings.maxResponseSizeKb` KB.
 *     Larger responses are truncated and annotated — Tauri does the
 *     truncation on the Rust side before the body crosses the IPC
 *     boundary; fetch path truncates in-process.
 *   - Returns an `ApiResponse`. Network failures, timeouts, and aborts
 *     surface as `status: 0` with distinguishing `statusText` so the
 *     UI can render the right banner.
 *   - Never throws — `HttpTester` just dispatches the returned
 *     `ApiResponse` into the store.
 */
import { invoke } from '@tauri-apps/api/core';
import type {
  ApiResponse,
  AuthConfig,
  Environment,
  HttpMethod,
  KeyValue,
  RequestBody,
} from '@/types/domain';
import type { RequestSettings } from '@/stores/app-store';
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

/**
 * Are we running inside the Tauri desktop shell?
 *
 * Tauri 2 sets `window.__TAURI_INTERNALS__` for any window that the
 * webview driver bootstraps. The legacy `window.__TAURI__` global is
 * kept alive by `withGlobalTauri: true` in `tauri.conf.json`, so we
 * check both for forward/backward compatibility.
 *
 * Detection happens at module load — flipping between modes within a
 * session would require a reload either way.
 */
const isTauri: boolean =
  typeof window !== 'undefined' &&
  (Boolean((window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) ||
    Boolean((window as { __TAURI__?: unknown }).__TAURI__));

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

/**
 * `invoke` wrapper that rejects with an AbortError when the caller's
 * `signal` fires. The underlying IPC call is NOT cancelled in Rust
 * (would need a future tauri release with native signal support),
 * but the JS-side promise rejects and the response is discarded by
 * `HttpTester` (whose `inFlightRef` mid-flight replacement prevents
 * the late response from clobbering the new one).
 */
async function invokeWithAbort<T>(
  cmd: string,
  args: Record<string, unknown>,
  signal: AbortSignal | undefined,
): Promise<T> {
  if (!signal) return invoke<T>(cmd, args);
  return new Promise<T>((resolve, reject) => {
    if (signal.aborted) {
      reject(new DOMException('aborted', 'AbortError'));
      return;
    }
    const onAbort = () => reject(new DOMException('aborted', 'AbortError'));
    signal.addEventListener('abort', onAbort, { once: true });
    invoke<T>(cmd, args)
      .then(
        (v) => {
          signal.removeEventListener('abort', onAbort);
          resolve(v);
        },
        (e) => {
          signal.removeEventListener('abort', onAbort);
          reject(e);
        },
      );
  });
}

/** Flatten a `Headers` object into a plain `Record<string, string>`.
 *  Multiple values for the same key (e.g. `Set-Cookie`) are joined
 *  with `, ` — the standard HTTP convention. */
function headersToObject(headers: Headers): Record<string, string> {
  const out: Record<string, string> = {};
  headers.forEach((value, key) => {
    if (out[key] === undefined) out[key] = value;
    else out[key] = `${out[key]}, ${value}`;
  });
  return out;
}

/** Same flattening but starting from `Array<[name, value]>` (Tauri). */
function tuplesToRecord(pairs: ReadonlyArray<readonly [string, string]>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of pairs) {
    if (out[k] === undefined) out[k] = v;
    else out[k] = `${out[k]}, ${v}`;
  }
  return out;
}

/** Convert a `HeadersInit` into a list of `[name, value]` tuples.
 *  Used by the Tauri path so we can serialize headers as JSON. */
function headersInitToTuples(h: HeadersInit): Array<[string, string]> {
  if (Array.isArray(h)) return h as Array<[string, string]>;
  if (typeof Headers !== 'undefined' && h instanceof Headers) {
    const out: Array<[string, string]> = [];
    h.forEach((v, k) => out.push([k, v]));
    return out;
  }
  if (h && typeof h === 'object') return Object.entries(h as Record<string, string>);
  return [];
}

/** UTF-8 byte length (matches what `Blob([s]).size` returns). */
function byteLength(s: string): number {
  return new Blob([s]).size;
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
// error shape builders — shared between transports so the UI sees the same
// banner regardless of which path ran.
// ---------------------------------------------------------------------------

function timeoutResponse(
  input: SendInput,
  url: string,
  durationMs: number,
): ApiResponse {
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

function abortedResponse(
  input: SendInput,
  url: string,
  durationMs: number,
): ApiResponse {
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
// Tauri path
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

async function sendViaTauri(input: SendInput): Promise<ApiResponse> {
  const built = buildFetchInit(input);
  const headers = headersInitToTuples(built.init.headers ?? {});
  const body =
    typeof built.init.body === 'string' && built.init.body.length > 0
      ? built.init.body
      : null;

  try {
    // Tauri 2 `invoke` doesn't expose `signal` on its InvokeOptions
    // shape in this version (added later in 2.x). Wrap the call in a
    // tiny Promise.race-ish helper so the caller's AbortSignal still
    // rejects this side — the Rust `reqwest` keeps running to
    // completion in the background but its result is discarded.
    const r = await invokeWithAbort<RustHttpResult>(
      'http_request',
      {
        method: input.method,
        url: built.url,
        headers,
        body,
        timeoutMs: input.settings.timeoutMs,
        followRedirects: input.settings.followRedirects,
        maxResponseSizeKb: input.settings.maxResponseSizeKb,
      },
      input.signal,
    );

    if (!r.ok || !r.response) {
      // Round-trip succeeded; the server-side request itself failed.
      const kind = r.error?.kind ?? 'network';
      const msg = r.error?.message ?? 'unknown';
      if (kind === 'timeout') {
        return timeoutResponse(input, built.url, 0);
      }
      return networkErrorResponse(input, built.url, 0, msg);
    }

    const resp = r.response;
    return {
      status: resp.status,
      statusText:
        resp.statusText ||
        (resp.status >= 200 && resp.status < 300 ? 'OK' : 'Error'),
      durationMs: resp.durationMs,
      sizeBytes: resp.sizeBytes,
      headers: tuplesToRecord(resp.headers),
      body: resp.body,
      bodyJson: tryParseJson(resp.body),
      contentType: resp.contentType,
      method: input.method,
      url: built.url,
    };
  } catch (err) {
    // invoke() rejects on: Tauri not actually wired (IPC failure),
    // deserialization mismatch, or the caller's AbortSignal aborting.
    // We surface abort distinctly; everything else falls back to the
    // generic Network Error banner so the UI stays informative.
    const name = (err as { name?: string })?.name;
    const msg = (err as Error)?.message ?? String(err);
    if (name === 'AbortError' || /abort/i.test(msg)) {
      return abortedResponse(input, built.url, 0);
    }
    console.error('[http.ts] tauri invoke failed:', err);
    return networkErrorResponse(input, built.url, 0, msg);
  }
}

// ---------------------------------------------------------------------------
// browser fetch path (original behavior, kept for the Vite-only dev mode)
// ---------------------------------------------------------------------------

async function sendViaFetch(input: SendInput): Promise<ApiResponse> {
  const built = buildFetchInit(input);
  const start = performance.now();

  // ----- AbortController composition -----
  const ctrl = new AbortController();
  let externalListener: (() => void) | undefined;
  if (input.signal) {
    if (input.signal.aborted) {
      ctrl.abort(input.signal.reason);
    } else {
      const onExternalAbort = () => ctrl.abort(input.signal!.reason);
      externalListener = onExternalAbort;
      input.signal.addEventListener('abort', onExternalAbort, { once: true });
    }
  }

  // ----- Timeout -----
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    ctrl.abort();
  }, input.settings.timeoutMs);

  const init: RequestInit = built.init;
  init.signal = ctrl.signal;
  init.redirect = input.settings.followRedirects ? 'follow' : 'manual';

  try {
    const res = await fetch(built.url, init);
    const durationMs = performance.now() - start;

    const maxBytes = Math.max(1024, input.settings.maxResponseSizeKb * 1024);
    const fullText = await res.text();
    const truncated = fullText.length > maxBytes;
    const text = truncated
      ? `${fullText.slice(0, maxBytes)}\n\n[truncated at ${input.settings.maxResponseSizeKb} KB]`
      : fullText;

    return {
      status: res.status,
      statusText: res.statusText || (res.ok ? 'OK' : 'Error'),
      durationMs,
      sizeBytes: byteLength(text),
      headers: headersToObject(res.headers),
      body: text,
      bodyJson: tryParseJson(text),
      contentType: res.headers.get('content-type') ?? '',
      method: input.method,
      url: built.url,
    };
  } catch (err) {
    const durationMs = performance.now() - start;
    const name = (err as { name?: string })?.name;
    if (name === 'AbortError') {
      if (timedOut) {
        return timeoutResponse(input, built.url, durationMs);
      }
      return abortedResponse(input, built.url, durationMs);
    }
    return networkErrorResponse(
      input,
      built.url,
      durationMs,
      (err as Error)?.message ?? String(err),
    );
  } finally {
    clearTimeout(timer);
    if (externalListener && input.signal) {
      input.signal.removeEventListener('abort', externalListener);
    }
  }
}

// ---------------------------------------------------------------------------
// public entry — picks the transport once based on the runtime context.
// ---------------------------------------------------------------------------

/**
 * Send the request described by `input` via `fetch` (browser) or the
 * Tauri `http_request` command (desktop shell). See module header.
 */
export async function sendRequest(input: SendInput): Promise<ApiResponse> {
  if (isTauri) {
    return sendViaTauri(input);
  }
  return sendViaFetch(input);
}
