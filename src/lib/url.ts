/**
 * URL helpers — pure, framework-free. Kept here so HttpTester, the
 * BaseUrlPicker, and the cURL exporter all share the same join /
 * resolution logic. If you change the resolution rules, change them
 * here once.
 */
import type { BaseUrlDefinition, EndpointDetail, Environment, Project } from '@/types/domain';

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
 *   1. `endpoint.baseUrlOverride`       — per-endpoint custom mode
 *   2. `endpoint.baseUrlDefinitionId`   — the def the endpoint points at
 *      (must be in `project.baseUrlDefinitions`); the URL is then
 *      `env.baseUrls[defId]`
 *   3. `project.defaultBaseUrlDefinitionId` → `env.baseUrls[defId]`
 *   4. `''`                              — nothing
 *
 * ## The override is three-state, and "present but empty" is meaningful
 *
 *   `null` / `undefined` — not in custom mode; fall through to the def chain.
 *   `''`                 — custom mode with no prefix: the URL field is the
 *                          whole address, so nothing is prepended to it.
 *   `'https://x'`        — custom mode with an explicit host prefix (legacy
 *                          shape; a full URL in the field still wins because
 *                          `joinBaseUrl` lets a path carrying a scheme win).
 *
 * This is the `double_option` patch convention doing real work: the caller
 * writes `baseUrlOverride: ''` to *record the choice*, not to clear it, and
 * `null` is the only thing that clears it. Collapsing `''` back into the def
 * chain is what made the picker's 自定义 row look like it did nothing.
 *
 * `env.baseUrls` is keyed by `BaseUrlDefinition.id`, NOT by name.
 * That keeps rename operations cheap and the resolution deterministic
 * even if two defs ever share a name. The env is taken from
 * `endpoint.environmentId` (the active env for the endpoint's other
 * variables) — when no env is selected, baseUrl resolves to ''.
 */
export function resolveEndpointBaseUrl(
  endpoint: Pick<EndpointDetail, 'environmentId' | 'baseUrlDefinitionId' | 'baseUrlOverride'>,
  env: Pick<Environment, 'baseUrls'> | undefined,
  project: Pick<Project, 'baseUrlDefinitions' | 'defaultBaseUrlDefinitionId'> | undefined,
): ResolvedBaseUrl {
  if (endpoint.baseUrlOverride !== null && endpoint.baseUrlOverride !== undefined) {
    return {
      baseUrl: endpoint.baseUrlOverride,
      source: 'override',
      definitionId: null,
      definitionName: null,
    };
  }
  const defId = endpoint.baseUrlDefinitionId ?? project?.defaultBaseUrlDefinitionId ?? null;
  if (defId && project) {
    const def = project.baseUrlDefinitions.find((d) => d.id === defId);
    if (def) {
      const url = env?.baseUrls[defId];
      if (url) {
        return {
          baseUrl: url,
          source: endpoint.baseUrlDefinitionId === defId ? 'definition' : 'default',
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

export interface ResolvedVars {
  value: string;
  /**
   * `{{name}}` markers that were **not** resolved, deduped by first
   * occurrence so the order is stable across re-renders (a warning pill
   * that reshuffles on every keystroke is worse than useless).
   *
   * This is the renderer-side twin of `ResolvedTemplate.missing` in
   * `crates/simple-post-mcp/src/variable_fill.rs`; the two must agree or
   * the frontend's "unresolved variables" hint and the MCP tool's
   * `missing_vars` report different things for the same endpoint.
   */
  missing: string[];
}

/**
 * Resolve a string of the form `{{varName}}` against a variable pool, and
 * report which markers could not be resolved. Accepts either an
 * `Environment` (in which case `env.variables` is used) or a raw
 * `Record<string,string>`.
 *
 * Unknown variables are left intact (e.g. `{{missing}}` stays as
 * `{{missing}}`) so the user can see what's missing in the request
 * preview instead of silently turning into `undefined`.
 *
 * The marker name only accepts `\w+` (`[A-Za-z0-9_]+`); `{{ my-var }}`
 * and `{{ a.b }}` pass through verbatim. Replacement is a **single,
 * non-recursive pass** — `{{{{a}}}}` becomes `{{a}}` and is not expanded
 * again. That mirrors `template_replace` / `is_word` in the Rust
 * resolver, and it is what stops a variable whose value happens to
 * contain `{{` from being expanded a second time (or looping).
 */
export function resolveVarsWithMissing(
  input: string,
  vars: Environment | Record<string, string> | undefined,
): ResolvedVars {
  if (!input) return { value: '', missing: [] };
  // `Environment` carries the variables under `.variables`; a raw record
  // is the map itself. Discriminate via `Environment.baseUrls` (a nested
  // object) — a plain Record<string,string> would yield `string` there.
  const map: Record<string, string> | undefined = isEnvironment(vars) ? vars.variables : vars;
  const missing: string[] = [];
  const seen = new Set<string>();
  const value = input.replace(/\{\{(\w+)\}\}/g, (_, key: string) => {
    const v = map?.[key];
    // Nullish check, not truthiness: a variable explicitly defined as
    // the empty string resolves to '' and must NOT be reported missing —
    // only an absent key leaves the marker in place.
    if (v !== undefined && v !== null) return v;
    if (!seen.has(key)) {
      seen.add(key);
      missing.push(key);
    }
    return `{{${key}}}`;
  });
  return { value, missing };
}

/**
 * `resolveVarsWithMissing`, discarding the missing list. Kept as the
 * name every existing caller imports — the two are one implementation,
 * so a `{{x}}` can never survive here while being reported there.
 */
export function resolveVars(
  input: string,
  vars: Environment | Record<string, string> | undefined,
): string {
  return resolveVarsWithMissing(input, vars).value;
}

function isEnvironment(v: Environment | Record<string, string> | undefined): v is Environment {
  return (
    !!v &&
    typeof v === 'object' &&
    typeof (v as Environment).baseUrls === 'object' &&
    !Array.isArray((v as Environment).baseUrls)
  );
}

// Re-export the picker-facing types so callers don't need a second
// import path.
export type { BaseUrlDefinition };
