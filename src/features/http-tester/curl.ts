/**
 * Endpoint → cURL string builder.
 *
 * Mirrors the conventions of the existing HttpTester (query params as URL
 * string, env var replacement via `{{var}}`). Body handling is intentionally
 * minimal: any non-`none` body mode with content becomes `--data-raw` and a
 * matching Content-Type header is added for `json` if not already present.
 */
import type { Endpoint, Environment, HttpMethod } from '@/types/domain';
import { resolveVars } from '@/lib/url';

export interface EndpointOverrides {
  /** Override `endpoint.url` (e.g. user has an unsaved draft). */
  url?: string;
  /** Override `endpoint.method` (e.g. user picked a different verb in the bar). */
  method?: HttpMethod;
}

/** POSIX-shell single-quote escape: 'foo' → 'foo', it's → 'it'\''s'. */
function shellQuote(s: string): string {
  return `'${s.replace(/'/g, "'\\''")}'`;
}

function buildQueryString(endpoint: Endpoint): string {
  const parts: string[] = [];
  for (const p of endpoint.params) {
    if (!p.enabled || !p.key) continue;
    parts.push(`${encodeURIComponent(p.key)}=${encodeURIComponent(p.value)}`);
  }
  return parts.join('&');
}

/**
 * Build a copy-pasteable cURL command for the given endpoint.
 *
 * If `overrides` is supplied, its `url` / `method` take precedence over the
 * stored values — used to reflect the current HttpTester draft before the
 * user clicks Save.
 */
export function endpointToCurl(
  endpoint: Endpoint,
  env: Environment | undefined,
  overrides?: EndpointOverrides,
): string {
  const method = overrides?.method ?? endpoint.method;
  const rawUrl = overrides?.url ?? endpoint.url;

  // 1. URL: resolve env vars, then append enabled query params.
  const resolvedUrl = resolveVars(rawUrl, env);
  const qs = buildQueryString(endpoint);
  const fullUrl = qs ? `${resolvedUrl}${resolvedUrl.includes('?') ? '&' : '?'}${qs}` : resolvedUrl;

  // 2. Headers: enabled, with {{var}} resolution. Auto-add Content-Type for json.
  const headerLines: string[] = [];
  const enabledHeaders = endpoint.headers.filter((h) => h.enabled && h.key);

  const body = endpoint.body;
  const hasBody = body.mode !== 'none' && body.content.length > 0;
  const isJson = body.mode === 'json';

  if (isJson && hasBody) {
    const hasCt = enabledHeaders.some((h) => h.key.toLowerCase() === 'content-type');
    if (!hasCt) {
      headerLines.push(`-H ${shellQuote('Content-Type: application/json')}`);
    }
  }

  for (const h of enabledHeaders) {
    headerLines.push(`-H ${shellQuote(`${h.key}: ${resolveVars(h.value, env)}`)}`);
  }

  // 3. Assemble.
  const lines: string[] = [`curl -X ${method} ${shellQuote(fullUrl)}`];
  for (const h of headerLines) lines.push(`  ${h}`);
  if (hasBody) {
    lines.push(`  --data-raw ${shellQuote(body.content)}`);
  }

  return lines.join(' \\\n');
}
