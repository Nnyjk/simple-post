/**
 * Shared builder for "what we'd send" — used by the real fetch
 * sender (`src/lib/http.ts`) and by the cURL exporter
 * (`src/features/http-tester/curl.ts`). Keeping both surfaces on a
 * single implementation prevents them from drifting apart: a header
 * that fetch sends but cURL omits (or vice versa) is exactly the
 * debugging surprise we want to avoid.
 *
 * Pure functions, no I/O. The browser's native `fetch` handles I/O;
 * this module only turns the user-editable request shape into the
 * URL + init pair (or cURL string) the runtime needs.
 */
import type {
  AuthConfig,
  BodyMode,
  Environment,
  HttpMethod,
  KeyValue,
  RequestBody,
} from '@/types/domain';
import { resolveVars } from './url';

/**
 * The inputs that affect the *wire shape* of a request. Settings
 * (timeout / redirects / size cap) and the caller's `AbortSignal`
 * intentionally live elsewhere — they only matter at execution time,
 * not at build time.
 */
export interface BuildInput {
  method: HttpMethod;
  url: string;
  params: KeyValue[];
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
  env?: Environment;
}

export interface BuiltFetch {
  /** Final URL — query params and apikey-in-query appended. */
  url: string;
  /**
   * `RequestInit` ready to hand to `fetch`. `signal` and `redirect`
   * are NOT set — the caller composes those on top.
   */
  init: RequestInit;
  /**
   * Echo of the body's content as a string (already URL/form-safe in
   * the form modes). Reserved for a future "show what we'd send"
   * preview surface; `sendRequest` does not currently use it.
   */
  bodyForLog: string;
}

// ---- internal helpers ----

/** Append enabled, non-empty params as a `?k=v&...` query string. */
function appendQueryString(url: string, params: readonly KeyValue[]): string {
  const parts: string[] = [];
  for (const p of params) {
    if (!p.enabled) continue;
    if (!p.key || p.value === '') continue; // spec: skip empty keys/values
    parts.push(
      `${encodeURIComponent(p.key)}=${encodeURIComponent(p.value)}`,
    );
  }
  if (parts.length === 0) return url;
  return `${url}${url.includes('?') ? '&' : '?'}${parts.join('&')}`;
}

/** Resolve `{{var}}` placeholders against the active env. */
function resolveTemplate(input: string, env: Environment | undefined): string {
  return resolveVars(input, env);
}

/**
 * Resolve all `{{var}}` placeholders inside an auth config against the
 * env. The returned config is a new object so the caller's input is
 * never mutated.
 */
function resolveAuth(
  auth: AuthConfig,
  env: Environment | undefined,
): AuthConfig {
  if (auth.type === 'bearer') {
    return { ...auth, bearer: resolveTemplate(auth.bearer ?? '', env) };
  }
  if (auth.type === 'basic' && auth.basic) {
    return {
      ...auth,
      basic: {
        username: resolveTemplate(auth.basic.username, env),
        password: resolveTemplate(auth.basic.password, env),
      },
    };
  }
  if (auth.type === 'apikey' && auth.apikey) {
    return {
      ...auth,
      apikey: {
        ...auth.apikey,
        key: resolveTemplate(auth.apikey.key, env),
        value: resolveTemplate(auth.apikey.value, env),
      },
    };
  }
  return auth;
}

/** Auto Content-Type for the given body mode (or `null` for `none`). */
function autoContentType(mode: BodyMode): string | null {
  switch (mode) {
    case 'json':
      return 'application/json';
    case 'form':
      return 'application/x-www-form-urlencoded';
    case 'raw':
      return 'text/plain';
    default:
      return null;
  }
}

interface OrderedHeader {
  key: string;
  value: string;
}

/**
 * Build the ordered header list: user headers first, then the auto
 * `Content-Type` (if missing) and `Accept`, then any auth-derived
 * header (`Authorization` or apikey-as-header). Duplicate-key check
 * is case-insensitive (HTTP header names are).
 *
 * `apikey.in_ === 'query'` is NOT emitted as a header — the caller
 * appends it to the URL via `appendApikeyToQuery`.
 */
function buildOrderedHeaders(
  input: BuildInput,
  auth: AuthConfig,
): OrderedHeader[] {
  const out: OrderedHeader[] = [];
  for (const h of input.headers) {
    if (!h.enabled || !h.key) continue;
    out.push({ key: h.key, value: resolveTemplate(h.value, input.env) });
  }

  // Auto Content-Type based on body.mode. Per spec, only added when
  // the user hasn't already set one (case-insensitive). We add it
  // whenever the mode implies a body — content emptiness is handled
  // separately by `init.body` assignment.
  const ct = autoContentType(input.body.mode);
  if (ct && !out.some((h) => h.key.toLowerCase() === 'content-type')) {
    out.push({ key: 'Content-Type', value: ct });
  }

  // Default Accept so the server knows we'd parse JSON cleanly.
  if (!out.some((h) => h.key.toLowerCase() === 'accept')) {
    out.push({ key: 'Accept', value: 'application/json' });
  }

  // Auth-derived header. Skip when the user already has the same
  // name (case-insensitive) — user wins for fetch since we use a
  // Record (last value kept) and for cURL we'd otherwise emit two
  // `-H 'Authorization: …'` lines, the second of which the server
  // would see as a header-list, not an override.
  if (auth.type === 'bearer' && auth.bearer) {
    if (!out.some((h) => h.key.toLowerCase() === 'authorization')) {
      out.push({ key: 'Authorization', value: `Bearer ${auth.bearer}` });
    }
  } else if (auth.type === 'basic' && auth.basic) {
    if (!out.some((h) => h.key.toLowerCase() === 'authorization')) {
      const { username, password } = auth.basic;
      out.push({ key: 'Authorization', value: `Basic ${b64Basic(username, password)}` });
    }
  } else if (auth.type === 'apikey' && auth.apikey && auth.apikey.in_ === 'header') {
    if (!out.some((h) => h.key.toLowerCase() === auth.apikey!.key.toLowerCase())) {
      out.push({ key: auth.apikey.key, value: auth.apikey.value });
    }
  }

  return out;
}

/**
 * Base64-encode `user:pass` for HTTP Basic auth. We route through
 * `TextEncoder` so credentials containing non-ASCII (e.g. Chinese
 * usernames, emoji passwords) are encoded as UTF-8 bytes before
 * base64 — the de-facto browser convention that Express and most
 * servers accept, since they decode the header as raw bytes.
 *
 * `btoa` is provided by the DOM lib, which is in the project's
 * tsconfig (`"lib": ["ES2022", "DOM", "DOM.Iterable"]`).
 */
function b64Basic(user: string, pass: string): string {
  const raw = `${user}:${pass}`;
  const bytes = new TextEncoder().encode(raw);
  let bin = '';
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]!);
  return btoa(bin);
}

/** Append an `apikey` config whose `in_` is `'query'` to the URL. */
function appendApikeyToQuery(url: string, auth: AuthConfig): string {
  if (auth.type !== 'apikey' || !auth.apikey || auth.apikey.in_ !== 'query') {
    return url;
  }
  const { key, value } = auth.apikey;
  if (!key) return url;
  return `${url}${url.includes('?') ? '&' : '?'}${encodeURIComponent(key)}=${encodeURIComponent(value)}`;
}

/** POSIX-shell single-quote escape: `it's` → `'it'\''s'`. */
function shellQuote(s: string): string {
  return `'${s.replace(/'/g, "'\\''")}'`;
}

/** Attach a body only when the mode says so AND content is non-empty. */
function hasBody(body: RequestBody): boolean {
  return body.mode !== 'none' && body.content.length > 0;
}

// ---- public API ----

/**
 * Build the `fetch(url, init)` pair for the given request. Pure — no
 * I/O, no state. The caller composes `signal` and `redirect` on top.
 */
export function buildFetchInit(input: BuildInput): BuiltFetch {
  const auth = resolveAuth(input.auth, input.env);
  const baseUrl = resolveTemplate(input.url, input.env);
  const urlWithParams = appendQueryString(baseUrl, input.params);
  const finalUrl = appendApikeyToQuery(urlWithParams, auth);
  const orderedHeaders = buildOrderedHeaders(input, auth);

  const headersRecord: Record<string, string> = {};
  for (const h of orderedHeaders) headersRecord[h.key] = h.value;

  const init: RequestInit = {
    method: input.method,
    headers: headersRecord,
  };
  if (hasBody(input.body)) {
    init.body = input.body.content;
  }

  return {
    url: finalUrl,
    init,
    bodyForLog: hasBody(input.body) ? input.body.content : '',
  };
}

/**
 * Build a copy-pasteable cURL command for the same request. Uses the
 * exact same URL / header / body derivation as `buildFetchInit` so
 * the two stay in lockstep.
 */
export function buildCurlCommand(input: BuildInput): string {
  const auth = resolveAuth(input.auth, input.env);
  const baseUrl = resolveTemplate(input.url, input.env);
  const urlWithParams = appendQueryString(baseUrl, input.params);
  const finalUrl = appendApikeyToQuery(urlWithParams, auth);
  const orderedHeaders = buildOrderedHeaders(input, auth);

  const lines: string[] = [
    `curl -X ${input.method.toUpperCase()} ${shellQuote(finalUrl)}`,
  ];
  for (const h of orderedHeaders) {
    lines.push(`  -H ${shellQuote(`${h.key}: ${h.value}`)}`);
  }
  if (hasBody(input.body)) {
    lines.push(`  --data-raw ${shellQuote(input.body.content)}`);
  }
  return lines.join(' \\\n');
}
