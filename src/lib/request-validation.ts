/**
 * Request format validation — pure, framework-free, and *explicitly
 * invoked*.
 *
 * WHY IT IS NOT DERIVED
 * ---------------------
 * The obvious implementation is a `useMemo` over the editor contents:
 * parse the JSON, show a badge when it fails. That is what this file
 * replaced, and it was wrong for two reasons:
 *
 *   1. Mid-edit JSON is *expected* to be broken. `{"a": 1,` is what a
 *      valid document looks like halfway through typing it, so a live
 *      check fires on nearly every keystroke and the badge becomes
 *      background noise the user learns to ignore.
 *   2. Nothing told the user *when* it was last checked. A verdict
 *      computed 400 ms ago is stale the moment the next key lands.
 *
 * So validation is an action, not a state: the three paths that make a
 * commitment — **Send**, **Save**, **格式化** — run it, and the result
 * is pinned until one of them runs again and succeeds. That matches how
 * a compiler works: you find out at build time, not on every character.
 *
 * SCOPE
 * -----
 * Format only. The one rule that exists is "json-mode body must parse".
 * Emptiness, param/header key sanity and URL shape are deliberately not
 * checked here — those are the backend's answer to report, and inventing
 * blocking rules the user did not ask for is how a validator starts
 * fighting the person using it.
 */
import type { RequestBody } from '@/types/domain';

/**
 * What failed, and where the user has to go to fix it.
 *
 * `tab` is what lets the caller *show* the problem instead of silently
 * doing nothing: pinning an error inside the body editor while the user
 * sits on the Params tab is the same invisibility this whole feature
 * exists to remove.
 */
export interface ValidationIssue {
  /** Which HTTP tab owns the offending content. */
  tab: 'body';
  /** One line, already phrased for display (no leading field name). */
  message: string;
}

/** What validation needs to see — the *effective* request, drafts included. */
export interface RequestValidationInput {
  body: RequestBody;
}

/**
 * Validate the effective request. Returns `null` when it is fine.
 *
 * Callers treat `null` as "clear any pinned error", so a passing run is
 * as meaningful as a failing one — it is what dismisses the badge.
 */
export function validateRequest(input: RequestValidationInput): ValidationIssue | null {
  const { body } = input;
  if (body.mode !== 'json') return null;
  const content = body.content.trim();
  // An empty json body is "nothing to send", not malformed JSON. Blocking
  // it here would make the badge a complaint about a field the user has
  // not filled in yet.
  if (!content) return null;
  try {
    JSON.parse(content);
    return null;
  } catch (e) {
    return { tab: 'body', message: (e as Error).message };
  }
}
