/**
 * URL helpers — pure, framework-free. Kept here so HttpTester, the
 * BaseUrlPicker, and the cURL exporter all share the same join /
 * resolution logic. If you change the resolution rules, change them
 * here once.
 */
import type { BaseUrlDefinition, EndpointDetail, Environment, Project } from '@/types/domain';

/** Where the effective baseUrl came from. Used by the picker to label
 *  the current selection (e.g. "项目默认服务" / "鉴权服务" / "自定义").
 *  `default` is the project's ★ — `Project.defaultBaseUrlDefinitionId`
 *  — reached because the endpoint named no service itself. */
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
 * Order of resolution (highest first):
 *   1. `endpoint.baseUrlOverride`      — per-endpoint 自定义 mode
 *   2. `endpoint.baseUrlDefinitionId`  — the service the endpoint named,
 *      else `Project.defaultBaseUrlDefinitionId`
 *   3. `env.moduleBaseUrls[defId][moduleId]` — the one and only value layer
 *   4. `''`                             — nothing
 *
 * ## The override is three-state, and "present but empty" is meaningful
 *
 *   `null` / `undefined` — not in custom mode; fall through to the service chain.
 *   `''`                 — custom mode with no prefix: the URL field is the
 *                          whole address, so nothing is prepended to it.
 *   `'https://x'`        — custom mode with an explicit host prefix (legacy
 *                          shape; a full URL in the field still wins because
 *                          `joinBaseUrl` lets a path carrying a scheme win).
 *
 * This is the `double_option` patch convention doing real work: the caller
 * writes `baseUrlOverride: ''` to *record the choice*, not to clear it, and
 * `null` is the only thing that clears it. Collapsing `''` back into the
 * service chain is what made the picker's 自定义 row look like it did
 * nothing.
 *
 * ## Services are project-level, values are per (env × module × service)
 *
 * `project.baseUrlDefinitions` is the project's whole list — every module
 * sees all of it, so naming a service is never a cross-module error. What
 * is module-specific is the *cell* the URL is read from, which is why
 * `moduleId` is a required argument: two modules pointing the same 服务
 * at two hosts is the case the model exists to express.
 *
 * There is **no fallback layer**. `baseUrls` is deprecated and never read;
 * an empty cell yields `''` / `'none'` rather than borrowing another
 * module's or another service's URL.
 *
 * `moduleId` is `null` for an endpoint that sits under no module (a deleted
 * node, or the tester's empty state). There is no cell for such a module,
 * so only step 1 can produce a value.
 *
 * `source` reports **which row selected the service**, not which cell
 * supplied the URL: a service the endpoint named is `'definition'`, the
 * project ★ reached by omission is `'default'`, and anything unresolved is
 * `'none'`.
 *
 * Mirrors `resolve_base_url_with` in
 * `crates/simple-post-mcp/src/variable_fill.rs`; the two must agree.
 */
export function resolveEndpointBaseUrl(
  endpoint: Pick<EndpointDetail, 'environmentId' | 'baseUrlDefinitionId' | 'baseUrlOverride'>,
  env: Pick<Environment, 'moduleBaseUrls'> | undefined,
  project: Pick<Project, 'baseUrlDefinitions' | 'defaultBaseUrlDefinitionId'> | undefined,
  /**
   * The module the endpoint sits under, resolved by the caller with
   * `moduleIdOfEndpoint` — it is a tree question, and this module
   * deliberately knows nothing about the tree.
   */
  moduleId: string | null,
): ResolvedBaseUrl {
  if (endpoint.baseUrlOverride !== null && endpoint.baseUrlOverride !== undefined) {
    return {
      baseUrl: endpoint.baseUrlOverride,
      source: 'override',
      definitionId: null,
      definitionName: null,
    };
  }
  // `null` (not chosen) falls through to the project ★. This is the one
  // place the project's default is read, and it is read exactly once.
  const defId = endpoint.baseUrlDefinitionId ?? project?.defaultBaseUrlDefinitionId ?? null;
  const def = defId ? project?.baseUrlDefinitions.find((d) => d.id === defId) : undefined;
  if (!def) return { baseUrl: '', source: 'none', definitionId: null, definitionName: null };
  const url = moduleId ? (env?.moduleBaseUrls?.[def.id]?.[moduleId] ?? '') : '';
  if (!url) return { baseUrl: '', source: 'none', definitionId: null, definitionName: null };
  return {
    baseUrl: url,
    source: endpoint.baseUrlDefinitionId ? 'definition' : 'default',
    definitionId: def.id,
    definitionName: def.name,
  };
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
 * Collapse runs of `/` in the **path** of a URL that has already been
 * joined *and* `{{var}}`-resolved, leaving every other byte alone.
 *
 * Joining happens before substitution — necessarily, because an
 * environment's own base URL may be templated — so `joinBaseUrl` cannot
 * know that `{{var}}` will expand to something already starting with `/`.
 * It dutifully inserts a separator, and the substitution then turns
 * `http://host:9200/{{var}}` into `http://host:9200//api`, which is a
 * different route to most servers (often a redirect or a 404). Only after
 * substitution is the truth known, which is why this is a separate pass
 * rather than something `joinBaseUrl` tries to guess at.
 *
 * Scope is deliberately narrow — a blind `replace(/\/{2,}/g, '/')` would
 * corrupt real URLs:
 *   - `scheme://` is not a path. `https://` must survive verbatim.
 *   - a leading `//` with **no** scheme is a protocol-relative
 *     reference (`//cdn.example.com/x`); its slashes carry meaning.
 *   - `?query` / `#fragment` are data, not path: a param carrying a
 *     nested URL (`?redirect=https://x//y`) must pass through untouched.
 *   - an unresolved `{{var}}` marker is text, not a separator, so it is
 *     reported by `missingVars` rather than being flattened away.
 *
 * Mirrors `collapse_url_slashes` in
 * `crates/simple-post-mcp/src/variable_fill.rs`; the two must agree or
 * the preview and the request that actually goes out describe different
 * URLs.
 */
export function collapseUrlSlashes(url: string): string {
  if (!url) return url ?? '';
  // `scheme://` and the protocol-relative `//` are heads, not path.
  const scheme = /^[a-z][a-z0-9+.-]*:\/\//i.exec(url);
  let head = '';
  let rest = url;
  if (scheme) {
    head = scheme[0];
    rest = url.slice(scheme[0].length);
  } else if (url.startsWith('//')) {
    head = '//';
    rest = url.slice(2);
  }
  // An authority never contains a `/` (it ends at the first one), so
  // whatever follows the head is safe to treat as path.
  const cut = rest.search(/[?#]/);
  const path = cut === -1 ? rest : rest.slice(0, cut);
  const tail = cut === -1 ? '' : rest.slice(cut);
  return head + path.replace(/\/{2,}/g, '/') + tail;
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
