/**
 * Endpoint → cURL string builder.
 *
 * Thin wrapper over the shared `buildCurlCommand` in
 * `@/lib/http-build` — kept here so callers (HttpTester's "copy as
 * cURL" button) don't have to switch import path.
 *
 * Mirrors the "store draft → effective request" flow: when an
 * `overrides` is passed, its `url` / `method` win over the stored
 * endpoint values, so the user can copy a cURL that reflects an
 * unsaved HttpTester draft.
 */
import type { Endpoint, Environment, HttpMethod } from '@/types/domain';
import { buildCurlCommand } from '@/lib/http-build';

export interface EndpointOverrides {
  /** Override `endpoint.url` (e.g. user has an unsaved draft). */
  url?: string;
  /** Override `endpoint.method` (e.g. user picked a different verb in the bar). */
  method?: HttpMethod;
}

/**
 * Build a copy-pasteable cURL command for the given endpoint.
 *
 * If `overrides` is supplied, its `url` / `method` take precedence over
 * the stored values — used to reflect the current HttpTester draft
 * before the user clicks Save.
 */
export function endpointToCurl(
  endpoint: Endpoint,
  env: Environment | undefined,
  overrides?: EndpointOverrides,
): string {
  const method = overrides?.method ?? endpoint.method;
  const url = overrides?.url ?? endpoint.url;
  return buildCurlCommand({
    method,
    url,
    params: endpoint.params,
    headers: endpoint.headers,
    body: endpoint.body,
    auth: endpoint.auth,
    env,
  });
}
