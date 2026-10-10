import { useMemo, useRef } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import { oneDark } from '@codemirror/theme-one-dark';
import type { BodyMode, RequestBody } from '@/types/domain';
import { Tabs } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
import { AlertTriangle, Wand2 } from 'lucide-react';
import { cn, tryFormatJson, type FormatResult } from '@/lib/utils';
import { useIsDark } from '@/lib/theme';
import { lightCodeTheme } from '@/lib/codemirror-themes';
import { gsap, useGSAP } from '@/lib/gsap';

interface Props {
  value: RequestBody;
  onChange: (body: RequestBody) => void;
  /**
   * A format error the parent has pinned, or `null`.
   *
   * The parent owns it because validation is an **action**, not a
   * derived value: it runs on Send / Save / 格式化 and its verdict is
   * held until one of those runs again (see `lib/request-validation`).
   * That is also why this component cannot clear the badge itself —
   * fixing the JSON by typing is not evidence that it parses.
   *
   * Kept out of `BodyContent`'s own state so it survives switching to
   * another HTTP tab and back: the error belongs to the request, not to
   * how long this component has been mounted.
   */
  validationError?: string | null;
  /**
   * Called after 格式化 with the parse outcome.
   *
   * The result travels instead of a bare `ok` flag, and the parent settles
   * its pinned error from it rather than re-running validation: by the time
   * this callback fires, the formatted content is in the draft but the
   * parent has not re-rendered, so re-checking there would validate the
   * content the user just failed to format and re-pin an error that is
   * already fixed.
   */
  onFormatResult?: (result: FormatResult) => void;
}

const BODY_TABS: { id: BodyMode; label: string }[] = [
  { id: 'none', label: 'none' },
  { id: 'json', label: 'json' },
  { id: 'form', label: 'form' },
  { id: 'raw', label: 'raw' },
];

export function BodyEditor({ value, onChange, validationError, onFormatResult }: Props) {
  const handleFormat = () => {
    const result = tryFormatJson(value.content);
    if (result.ok) {
      onChange({ ...value, content: result.formatted });
    }
    // Report either way: the parent turns a pass into "clear the badge"
    // and a failure into "pin it", which is exactly the same rule Send
    // and Save follow.
    onFormatResult?.(result);
  };

  return (
    <div className="px-4 py-3">
      <div className="mb-2 flex items-center gap-2">
        <Tabs
          items={BODY_TABS.map((t) => ({ id: t.id, label: t.label }))}
          value={value.mode}
          onChange={(id) => {
            onChange({ ...value, mode: id as BodyMode });
          }}
        />
        <span className="text-[10px] text-muted-foreground">
          {value.mode === 'json' && 'Content-Type: application/json'}
          {value.mode === 'form' && 'Content-Type: application/x-www-form-urlencoded'}
          {value.mode === 'raw' && 'Content-Type: text/plain'}
        </span>
        <Button
          variant="outline"
          size="sm"
          className="ml-auto h-7"
          disabled={value.mode !== 'json' || !value.content.trim()}
          onClick={handleFormat}
          data-testid="format-body-btn"
        >
          <Wand2 className="h-3.5 w-3.5" />
          格式化
        </Button>
      </div>

      {value.mode === 'none' ? (
        <div className="flex h-32 items-center justify-center rounded-md border border-dashed border-border text-sm text-muted-foreground">
          此请求无 body
        </div>
      ) : (
        <BodyContent value={value} onChange={onChange} validationError={validationError} />
      )}
    </div>
  );
}

function BodyContent({ value, onChange, validationError }: Props) {
  const isDark = useIsDark();
  const pillRef = useRef<HTMLDivElement>(null);

  // An error about a body you can no longer see (mode switched to
  // `form`) would be a badge in a different country. Anything that
  // leaves the json editor retires it — the next Send / Save re-checks
  // whatever is there now.
  const shownError = value.mode === 'json' ? (validationError ?? null) : null;

  // GSAP owns visibility for the same reason it owns the transition:
  // the element is never conditionally unmounted while an error is
  // showing, so a re-run of the same verdict can't race the ref.
  useGSAP(() => {
    const el = pillRef.current;
    if (!el || !shownError) return;
    const mm = gsap.matchMedia();
    mm.add('(prefers-reduced-motion: no-preference)', () => {
      gsap.fromTo(
        el,
        { autoAlpha: 0, y: -8, scale: 0.96 },
        { autoAlpha: 1, y: 0, scale: 1, duration: 0.28, ease: 'power3.out' },
      );
      // One shake, then it stays put. The motion has to mean "look at
      // me now" — a badge that keeps moving stops being read.
      gsap.fromTo(
        el,
        { x: 0 },
        {
          keyframes: [
            { x: -3, duration: 0.06 },
            { x: 3, duration: 0.06 },
            { x: -2, duration: 0.06 },
            { x: 0, duration: 0.06 },
          ],
        },
      );
    });
    return () => mm.revert();
  }, [shownError]);

  const hintText = useMemo(
    () => (shownError ? `JSON 格式有误：${shownError}` : null),
    [shownError],
  );

  return (
    <div className="relative">
      <div className="overflow-hidden rounded-md border border-border">
        <CodeMirror
          value={value.content}
          onChange={(v) => onChange({ ...value, content: v })}
          extensions={value.mode === 'json' ? [json()] : []}
          theme={isDark ? oneDark : lightCodeTheme}
          basicSetup={{
            lineNumbers: true,
            foldGutter: true,
            highlightActiveLine: true,
            highlightActiveLineGutter: true,
          }}
          height="280px"
          className="text-sm"
        />
      </div>

      {/*
        Floated at the top of the editor rather than stacked under it:
        the 280 px CodeMirror used to sit between the user and the badge,
        which is why a JSON error went unnoticed while typing. It stays
        pinned — no timeout, no dismissal — and leaves only when a
        validation run passes. `pointer-events-auto` costs one small dead
        zone in the editor's top-right corner and buys the untruncated
        message on hover, which matters exactly when the message is an
        error the user has to act on.
      */}
      {hintText && (
        <div
          ref={pillRef}
          role="status"
          aria-live="polite"
          className={cn(
            'absolute right-2 top-2 z-10 flex max-w-[min(420px,60%)] items-center gap-1.5',
            'rounded-full border px-2.5 py-1 text-[11px] shadow-md',
            // Solid destructive, not a tinted surface. `--destructive-foreground`
            // is 98% white in BOTH themes (it is the colour meant to sit *on*
            // red), so pairing it with a near-white surface — which is what
            // `bg-background/85` is in light mode — produced white text on a
            // white editor: the badge vanished outright in light mode and, in
            // dark mode, read as plain white text with a 40%-opacity border
            // and no error signal at all. `destructive` + its own foreground
            // is the pairing the tokens were designed for, and it resolves
            // identically in both themes because nothing overrides either.
            'border-destructive bg-destructive text-destructive-foreground',
          )}
          title={hintText}
          data-testid="json-status-pill"
        >
          <AlertTriangle className="h-3.5 w-3.5 shrink-0" aria-hidden />
          <span className="truncate font-mono">{hintText}</span>
        </div>
      )}
    </div>
  );
}
