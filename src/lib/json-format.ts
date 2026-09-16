// Best-effort JSON pretty-printer. Returns a tagged result so callers can
// distinguish success from parse errors without try/catch noise.

export type FormatResult =
  | { ok: true; formatted: string }
  | { ok: false; error: string };

export function tryFormatJson(input: string, indent = 2): FormatResult {
  const s = input.trim();
  if (!s) return { ok: false, error: '内容为空' };
  try {
    const parsed = JSON.parse(s);
    return { ok: true, formatted: JSON.stringify(parsed, null, indent) };
  } catch (e) {
    return { ok: false, error: (e as Error).message };
  }
}
