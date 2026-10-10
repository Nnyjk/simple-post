/**
 * Lazy boundary for the request-body editor.
 *
 * The editor is CodeMirror: 425 kB raw / 139 kB gzip, the single largest
 * dependency in the app. `HttpTester` used to import it statically, which
 * made the whole CodeMirror chunk a static dependency of the app entry — the
 * browser had to fetch and parse all of it before React could mount a
 * single pixel, even though the body tab is not the default view.
 *
 * This wrapper keeps `HttpTester`'s call site unchanged (same props, same
 * component name) and moves the cost behind a chunk boundary. The chunk is
 * still pre-warmed during the bootstrap window from `App.tsx`, so on the
 * common path — a returning user with an endpoint tab restored — the editor
 * is normally already resolved by the time this tab is visible.
 */
import { Suspense, lazy } from 'react';
import type { RequestBody } from '@/types/domain';
import type { FormatResult } from '@/lib/utils';

const BodyEditorImpl = lazy(() => import('./BodyEditor').then((m) => ({ default: m.BodyEditor })));

export function BodyEditor({
  value,
  onChange,
  validationError,
  onFormatResult,
}: {
  value: RequestBody;
  onChange: (body: RequestBody) => void;
  /** Pinned format error, owned by HttpTester — see BodyEditor's props. */
  validationError?: string | null;
  onFormatResult?: (result: FormatResult) => void;
}) {
  return (
    <Suspense fallback={<EditorSkeleton />}>
      <BodyEditorImpl
        value={value}
        onChange={onChange}
        validationError={validationError}
        onFormatResult={onFormatResult}
      />
    </Suspense>
  );
}

/**
 * Holds the editor's full height while the chunk streams in. A centred
 * spinner would collapse the pane and then snap back, which reads as a
 * layout glitch on a tab the user just opened.
 */
function EditorSkeleton() {
  return <div className="h-full w-full animate-pulse bg-muted/30" aria-busy="true" />;
}
