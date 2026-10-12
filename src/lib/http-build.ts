/**
 * Shared builder for "what we'd send" — the single construction core for
 * every surface that has to agree on the wire shape: the real sender
 * (`src/lib/http.ts`) and the copy-as-cURL preview.
 *
 * The layering is deliberate and one-directional:
 *
 *   prepareRequest()   → the one place a request is turned into a
 *                        URL + ordered headers + body (+ unresolved
 *                        variables). Nothing else assembles one.
 *   buildFetchInit()   → adapter over prepareRequest for `fetch`
 *   buildCurlCommand() → adapter over prepareRequest for a shell line
 *
 * That inversion is the point. The cURL preview used to be a hand-kept
 * "mirror" of what the backend sent; two implementations of the same
 * rules drift, and the user sees it as "copy as cURL" producing a
 * request that is not the one they just sent. Deriving both from one
 * result makes that class of bug unrepresentable rather than merely
 * unlikely — preview and execution are now literally the same product.
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
import { collapseUrlSlashes, joinBaseUrl, resolveVarsWithMissing } from './url';

/**
 * The inputs that affect the *wire shape* of a request. Settings
 * (timeout / redirects / size cap) and the caller's `AbortSignal`
 * intentionally live elsewhere — they only matter at execution time,
 * not at build time.
 *
 * `url` is the **whole** URL: there is no base-URL slot, so every
 * caller has to join one first. Use [`prepareRequest`] when a base URL
 * and a URL path are the natural inputs; this shape stays as the
 * compatibility surface for the existing fetch/cURL adapters, which
 * treat it as `baseUrl: ''`.
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

// ---------------------------------------------------------------------------
// prepareRequest — the one construction core
// ---------------------------------------------------------------------------

export interface PrepareRequestInput {
  method: HttpMethod;
  /**
   * URL **path** part, without any base URL. May carry `{{var}}`.
   * A path that already has its own scheme (`https://…`) wins over
   * `baseUrl`, which is what makes the endpoint's 自定义 mode work: the
   * user types the whole address and the picker still has a value
   * selected.
   */
  urlPath: string;
  /** Resolved base URL. Empty string means "no base URL". */
  baseUrl: string;
  params: KeyValue[];
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
  env?: Environment;
}

export interface PreparedRequest {
  method: HttpMethod;
  /**
   * The complete URL: `join(baseUrl, urlPath)` → `{{var}}` resolution →
   * path slash-collapse → param values resolved and appended →
   * apikey-in-query, in that order.
   * Joining *before* resolving means a `{{var}}` inside the environment's
   * own base URL is resolved too. The collapse comes after resolving
   * because it is the substitution, not the join, that turns the join's
   * separator into a `//` when a variable expands to a value starting
   * with `/`. Params are merged last because their values are resolved on
   * the way in (see `appendQueryString`), so by the time they are encoded
   * no marker can survive into the URL.
   */
  url: string;
  /**
   * Ordered header tuples. A list, not a `Record`, because HTTP allows
   * a repeated header name and `Record` would silently collapse them —
   * the wire shape has to survive to the backend, which takes
   * `Vec<(String, String)>`.
   */
  headers: Array<[string, string]>;
  /**
   * Body content, or `null` when `mode === 'none'` or the authored content
   * is empty. Templated like every other field: `{{var}}` resolves against
   * the active environment.
   *
   * The inclusion test runs on the **authored** content, before
   * templating, which mirrors Rust's `resolve_endpoint`. So a body whose
   * only content is a variable that resolves to `""` ships an empty string
   * rather than being dropped — a request that was deliberately given a
   * body still gets one.
   *
   * This used to ship verbatim with the markers intact. The reason given
   * was that substituting into a JSON payload can produce invalid JSON,
   * which is real, but it left the app disagreeing with its own backend
   * (`resolve_endpoint` has always templated) and left `{{x}}` going out
   * to the server as data. If a variable resolves to something that
   * breaks the payload, that is the user's value to fix, and
   * `missingVars` covers the case where it did not resolve at all.
   */
  body: string | null;
  /**
   * Every `{{name}}` left unresolved anywhere in URL / param values /
   * headers / body / auth, deduped and in first-occurrence order. The set
   * is aligned with `resolve_endpoint`'s `missing_vars` in Rust; the
   * *order* is renderer-local (URL, params, headers, body, auth) because
   * nothing consumes it but this app's own warning UI.
   */
  missingVars: string[];
}

// ---- internal helpers ----

/**
 * Accumulates unresolved `{{name}}` markers across every field a request
 * touches, deduping by first occurrence. A `Set` for membership plus an
 * array for order: `Set` alone would give the same set but an order that
 * depends on insertion internals, and the order is what makes the
 * "these variables are missing" hint stable while the user types.
 */
interface MissingTracker {
  add(names: readonly string[]): void;
  list(): string[];
}

function createMissingTracker(): MissingTracker {
  const seen = new Set<string>();
  const names: string[] = [];
  return {
    add(chunk) {
      for (const n of chunk) {
        if (seen.has(n)) continue;
        seen.add(n);
        names.push(n);
      }
    },
    // Copy: `PreparedRequest` is a value object handed to callers, so it
    // must not hand out the array we keep appending to.
    list: () => names.slice(),
  };
}

/**
 * Append enabled, non-empty params as a `?k=v&...` query string.
 *
 * A param value is a template like every other field: `{{userId}}`
 * resolves against the active environment before it is encoded. Mirrors
 * Rust's `resolve_endpoint`, which substitutes the **value** only and
 * passes `key` through verbatim — a variable in a key is not something
 * the backend resolves, so resolving it here would make the cURL preview
 * and the actual send disagree, which is the one thing this module
 * exists to make unrepresentable.
 *
 * Disabled params are skipped before substitution for the same reason
 * headers are: they never ship, so their variables are not this
 * request's problem and reporting them would be a false alarm.
 */
function appendQueryString(
  url: string,
  params: readonly KeyValue[],
  env: Environment | undefined,
  missing: MissingTracker,
): string {
  const parts: string[] = [];
  for (const p of params) {
    if (!p.enabled) continue;
    const value = resolveVarsWithMissing(p.value, env);
    missing.add(value.missing);
    // Empty check runs on the *resolved* value: a variable that resolves
    // to "" drops the param, exactly as a hand-typed empty value does.
    if (!p.key || value.value === '') continue; // spec: skip empty keys/values
    parts.push(`${encodeURIComponent(p.key)}=${encodeURIComponent(value.value)}`);
  }
  if (parts.length === 0) return url;
  return `${url}${url.includes('?') ? '&' : '?'}${parts.join('&')}`;
}

/**
 * Resolve all `{{var}}` placeholders inside an auth config against the
 * env, recording any that stayed unresolved. The returned config is a
 * new object so the caller's input is never mutated — the auth config
 * lives in the Zustand store and a send must not write to it.
 *
 * Only the fields that end up on the wire are scanned, and only for the
 * selected `kind`: a stale `bearer` string left on a Basic config is not
 * part of this request and reporting its variables would be noise.
 */
function resolveAuth(
  auth: AuthConfig,
  env: Environment | undefined,
  missing: MissingTracker,
): AuthConfig {
  if (auth.kind === 'bearer') {
    const r = resolveVarsWithMissing(auth.bearer ?? '', env);
    missing.add(r.missing);
    return { ...auth, bearer: r.value };
  }
  if (auth.kind === 'basic' && auth.basic) {
    const u = resolveVarsWithMissing(auth.basic.username, env);
    const p = resolveVarsWithMissing(auth.basic.password, env);
    missing.add(u.missing);
    missing.add(p.missing);
    return {
      ...auth,
      basic: { username: u.value, password: p.value },
    };
  }
  if (auth.kind === 'apikey' && auth.apikey) {
    const k = resolveVarsWithMissing(auth.apikey.key, env);
    const v = resolveVarsWithMissing(auth.apikey.value, env);
    missing.add(k.missing);
    missing.add(v.missing);
    return {
      ...auth,
      apikey: { ...auth.apikey, key: k.value, value: v.value },
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

/**
 * Build the ordered header list: user headers first, then the auto
 * `Content-Type` (if missing) and `Accept`, then any auth-derived
 * header (`Authorization` or apikey-as-header). Duplicate-key check
 * is case-insensitive (HTTP header names are).
 *
 * `apikey.inLocation === 'query'` is NOT emitted as a header — the caller
 * appends it to the URL via `appendApikeyToQuery`.
 */
function buildOrderedHeaders(
  headers: readonly KeyValue[],
  bodyMode: BodyMode,
  auth: AuthConfig,
  env: Environment | undefined,
  missing: MissingTracker,
): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  for (const h of headers) {
    if (!h.enabled || !h.key) continue;
    const r = resolveVarsWithMissing(h.value, env);
    missing.add(r.missing);
    out.push([h.key, r.value]);
  }

  // Auto Content-Type based on body.mode. Per spec, only added when
  // the user hasn't already set one (case-insensitive). We add it
  // whenever the mode implies a body — content emptiness is handled
  // separately by `init.body` assignment.
  const ct = autoContentType(bodyMode);
  if (ct && !out.some(([k]) => k.toLowerCase() === 'content-type')) {
    out.push(['Content-Type', ct]);
  }

  // Default Accept so the server knows we'd parse JSON cleanly.
  if (!out.some(([k]) => k.toLowerCase() === 'accept')) {
    out.push(['Accept', 'application/json']);
  }

  // Auth-derived header. Skip when the user already has the same
  // name (case-insensitive) — user wins for fetch since we use a
  // Record (last value kept) and for cURL we'd otherwise emit two
  // `-H 'Authorization: …'` lines, the second of which the server
  // would see as a header-list, not an override.
  if (auth.kind === 'bearer' && auth.bearer) {
    if (!out.some(([k]) => k.toLowerCase() === 'authorization')) {
      out.push(['Authorization', `Bearer ${auth.bearer}`]);
    }
  } else if (auth.kind === 'basic' && auth.basic) {
    if (!out.some(([k]) => k.toLowerCase() === 'authorization')) {
      const { username, password } = auth.basic;
      out.push(['Authorization', `Basic ${b64Basic(username, password)}`]);
    }
  } else if (auth.kind === 'apikey' && auth.apikey && auth.apikey.inLocation === 'header') {
    if (!out.some(([k]) => k.toLowerCase() === auth.apikey!.key.toLowerCase())) {
      out.push([auth.apikey.key, auth.apikey.value]);
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

/** Append an `apikey` config whose `inLocation` is `'query'` to the URL. */
function appendApikeyToQuery(url: string, auth: AuthConfig): string {
  if (auth.kind !== 'apikey' || !auth.apikey || auth.apikey.inLocation !== 'query') {
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
 * Turn the user-editable request shape into everything a transport needs.
 * This is the only function in the app that assembles a request.
 *
 * Order of operations (the order matters at each step):
 *
 *   1. join `baseUrl` + `urlPath` — a path with its own scheme wins, so
 *      a 自定义 full URL is never prefixed;
 *   2. resolve `{{var}}` over the **joined** URL, so a variable inside
 *      the environment's own base URL resolves too;
 *   3. resolve `{{var}}` in each enabled param's **value**, then append
 *      the survivors as a query string (encoding them, and choosing `?`
 *      vs `&` off the current URL);
 *   4. append an apikey configured for the query location;
 *   5. build the ordered header list;
 *   6. decide body inclusion (`mode !== 'none'` **and** non-empty
 *      authored content), then template the body.
 *
 * Every `{{var}}` that cannot be resolved is recorded, wherever it was
 * written — URL, param value, header, auth, or body — and every field
 * that goes on the wire is templated, including the body.
 *
 * Two of those were not always true, and both were the same kind of bug:
 * the field looked handled, the app and its own backend disagreed, and a
 * `{{x}}` went out to the server silently encoded or literal.
 *
 *   - Param values were exempt on the theory that a marker percent-encoded
 *     into the query string was "already visible there". It was not:
 *     `%7B%7Bx%7D%7D` is no more readable than `{{x}}`.
 *   - The body shipped verbatim, on the theory that substituting into JSON
 *     can produce invalid JSON. Real, but it left `{{x}}` in the payload
 *     as data and the frontend disagreeing with `resolve_endpoint`, which
 *     has always templated every field.
 */
export function prepareRequest(input: PrepareRequestInput): PreparedRequest {
  const missing = createMissingTracker();

  const auth = resolveAuth(input.auth, input.env, missing);

  const joined = joinBaseUrl(input.baseUrl, input.urlPath);
  const url = resolveVarsWithMissing(joined, input.env);
  missing.add(url.missing);
  // After substitution, never before: `{{var}}` → `/api` is what turns the
  // separator `joinBaseUrl` inserted into a `//`, so this is the first
  // point where the real path is known. Query/fragment data is left alone
  // by `collapseUrlSlashes`, which is why params can still be appended
  // after it.
  const normalized = collapseUrlSlashes(url.value);

  const withParams = appendQueryString(normalized, input.params, input.env, missing);
  const finalUrl = appendApikeyToQuery(withParams, auth);

  const headers = buildOrderedHeaders(input.headers, input.body.mode, auth, input.env, missing);

  // Decide inclusion on the authored content, then template it — the same
  // order Rust's `resolve_endpoint` uses. Checking inclusion *after*
  // templating would silently drop a request whose body is a variable that
  // resolves to the empty string.
  const body = hasBody(input.body) ? resolveVarsWithMissing(input.body.content, input.env) : null;
  if (body !== null) missing.add(body.missing);

  return {
    method: input.method,
    url: finalUrl,
    headers,
    body: body === null ? null : body.value,
    missingVars: missing.list(),
  };
}

/** `BuildInput` → `PrepareRequestInput`, with an empty base URL.
 *
 *  `BuildInput.url` has always been the complete URL, so the compat
 *  adapters below treat it as a path with nothing to join. `joinBaseUrl`
 *  is the identity on that input (an empty base returns the path, and a
 *  path with a scheme is returned as-is), which is what keeps
 *  `buildFetchInit` / `buildCurlCommand` byte-identical to before.
 */
function fromBuildInput(input: BuildInput): PrepareRequestInput {
  return {
    method: input.method,
    urlPath: input.url,
    baseUrl: '',
    params: input.params,
    headers: input.headers,
    body: input.body,
    auth: input.auth,
    env: input.env,
  };
}

/**
 * Build the `fetch(url, init)` pair for the given request. Pure — no
 * I/O, no state. The caller composes `signal` and `redirect` on top.
 *
 * A thin adapter over [`prepareRequest`]. Headers collapse back into a
 * `Record` here because that is `fetch`'s natural shape, which means a
 * repeated header name keeps only the last value — a pre-existing
 * limitation of this entry point, and the reason the tuple-preserving
 * `sendPreparedRequest` path exists for the backend transport.
 */
export function buildFetchInit(input: BuildInput): BuiltFetch {
  const prepared = prepareRequest(fromBuildInput(input));

  const headersRecord: Record<string, string> = {};
  for (const [k, v] of prepared.headers) headersRecord[k] = v;

  const init: RequestInit = {
    method: input.method,
    headers: headersRecord,
  };
  if (prepared.body !== null) {
    init.body = prepared.body;
  }

  return {
    url: prepared.url,
    init,
    bodyForLog: prepared.body ?? '',
  };
}

/**
 * Render an already-prepared request as a copy-pasteable cURL command.
 *
 * THE ONE PLACE A cURL STRING IS ASSEMBLED. `buildCurlCommand` below is
 * this same function behind a `BuildInput` adapter, which is the point:
 * a preview that re-derives the URL, headers and body is a preview that
 * can describe a request nobody sent. That is not hypothetical — the
 * preview used to be built independently of Send and emitted
 * `curl -X POST '/cashier'` for a request that had actually gone to
 * `https://host/cashier`, because only the send path joined the base
 * URL.
 *
 * Everything read here is already final: `url` is joined and
 * `{{var}}`-resolved, `headers` is the ordered wire list including the
 * auth-derived `Authorization`, and `body` is the exact string that
 * would be written to the socket.
 */
export function preparedToCurl(prepared: PreparedRequest): string {
  const lines: string[] = [`curl -X ${prepared.method.toUpperCase()} ${shellQuote(prepared.url)}`];
  // A list, not a record: `prepared.headers` keeps duplicate names, which
  // is legal on the wire and which one `-H` per line reproduces.
  for (const [k, v] of prepared.headers) {
    lines.push(`  -H ${shellQuote(`${k}: ${v}`)}`);
  }
  if (prepared.body !== null) {
    lines.push(`  --data-raw ${shellQuote(prepared.body)}`);
  }
  return lines.join(' \\\n');
}

/**
 * Build a copy-pasteable cURL command for the same request. A thin
 * adapter over [`prepareRequest`] → [`preparedToCurl`], so the emitted URL,
 * headers and body are by construction the ones a real send would use —
 * the two can no longer disagree.
 */
export function buildCurlCommand(input: BuildInput): string {
  return preparedToCurl(prepareRequest(fromBuildInput(input)));
}
