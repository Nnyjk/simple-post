/**
 * URL helpers — pure, framework-free. Kept here so HttpTester, the
 * BaseUrlPicker, and the cURL exporter all share the same join /
 * resolution logic. If you change the resolution rules, change them
 * here once.
 */
import type { BaseUrlDefinition, Endpoint, Environment, Project } from '@/types/domain';

/** Where the effective baseUrl came from. Used by the picker to label
 *  the current selection (e.g. "项目默认" / "鉴权服务" / "自定义"). */
export type BaseUrlSource = 'override' | 'definition' | 'default' | 'none';

export interface ResolvedBaseUrl {
  baseUrl: string;
  source: BaseUrlSource;
  /** The id of the baseUrl definition that contributed the value, if any. */
  definitionId: string | null;
  /** Human-readable name of the contributing definition. */
  definitionName: string | null;
}

/**
 * Resolve the effective baseUrl for an endpoint.
 *
 * Precedence (highest first):
 *   1. `endpoint.baseUrlOverride`       — per-endpoint custom host
 *   2. `endpoint.baseUrlDefinitionId`   — the def the endpoint points at
 *      (must be in `project.baseUrlDefinitions`); the URL is then
 *      `env.baseUrls[defId]`
 *   3. `project.defaultBaseUrlDefinitionId` → `env.baseUrls[defId]`
 *   4. `''`                              — nothing
 *
 * `env.baseUrls` is keyed by `BaseUrlDefinition.id`, NOT by name.
 * That keeps rename operations cheap and the resolution deterministic
 * even if two defs ever share a name. The env is taken from
 * `endpoint.environmentId` (the active env for the endpoint's other
 * variables) — when no env is selected, baseUrl resolves to ''.
 */
export function resolveEndpointBaseUrl(
  endpoint: Pick<
    Endpoint,
    'environmentId' | 'baseUrlDefinitionId' | 'baseUrlOverride'
  >,
  env: Pick<Environment, 'baseUrls'> | undefined,
  project:
    | Pick<Project, 'baseUrlDefinitions' | 'defaultBaseUrlDefinitionId'>
    | undefined,
): ResolvedBaseUrl {
  if (endpoint.baseUrlOverride && endpoint.baseUrlOverride.length > 0) {
    return {
      baseUrl: endpoint.baseUrlOverride,
      source: 'override',
      definitionId: null,
      definitionName: null,
    };
  }
  const defId =
    endpoint.baseUrlDefinitionId ?? project?.defaultBaseUrlDefinitionId ?? null;
  if (defId && project) {
    const def = project.baseUrlDefinitions.find((d) => d.id === defId);
    if (def) {
      const url = env?.baseUrls[defId];
      if (url) {
        return {
          baseUrl: url,
          source:
            endpoint.baseUrlDefinitionId === defId
              ? 'definition'
              : 'default',
          definitionId: def.id,
          definitionName: def.name,
        };
      }
    }
  }
  return { baseUrl: '', source: 'none', definitionId: null, definitionName: null };
}

/**
 * Concatenate a baseUrl and a request path safely. Treats the path as
 * already-absolute if it carries a scheme, and collapses duplicate
 * slashes at the join boundary.
 *
 *   joinBaseUrl('https://api.example.com', '/users/1')
 *     → 'https://api.example.com/users/1'
 *   joinBaseUrl('https://api.example.com/', 'users/1')
 *     → 'https://api.example.com/users/1'
 *   joinBaseUrl('', '/users/1')
 *     → '/users/1'
 *   joinBaseUrl('https://api.example.com', 'https://other/')
 *     → 'https://other/'  (the path wins — it carries its own scheme)
 */
export function joinBaseUrl(baseUrl: string, path: string): string {
  const trimmedPath = path ?? '';
  if (!trimmedPath) return baseUrl ?? '';
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(trimmedPath)) return trimmedPath;
  const trimmedBase = (baseUrl ?? '').replace(/\/+$/, '');
  if (!trimmedBase) return trimmedPath;
  if (trimmedPath.startsWith('/')) return trimmedBase + trimmedPath;
  return `${trimmedBase}/${trimmedPath}`;
}

/**
 * Resolve a string of the form `{{varName}}` against a `Record<string,string>`.
 * Unknown variables are left intact (e.g. `{{missing}}` stays as `{{missing}}`)
 * so the user can see what's missing in the request preview instead of
 * silently turning into `undefined`.
 */
export function resolveVars(
  input: string,
  vars: Record<string, string> | undefined,
): string {
  if (!input) return '';
  return input.replace(/\{\{(\w+)\}\}/g, (_, key: string) => {
    const v = vars?.[key];
    return v ?? `{{${key}}}`;
  });
}

// Re-export the picker-facing types so callers don't need a second
// import path.
export type { BaseUrlDefinition };
