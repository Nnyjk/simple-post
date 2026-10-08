/**
 * Lazy boundary for the endpoint docs editor.
 *
 * `marked` is 36 kB raw / 11 kB gzip and is only reachable from the docs
 * tab. Statically importing it put the markdown chunk in the entry's
 * critical path for every user, including those who never open docs.
 *
 * Same contract as `DocsEditor` (string in, string out); `HttpTester` is
 * unaware that the implementation is now behind a chunk boundary.
 */
import { Suspense, lazy } from 'react';

const DocsEditorImpl = lazy(() => import('./DocsEditor').then((m) => ({ default: m.DocsEditor })));

export function DocsEditor({
  value,
  onChange,
}: {
  value: string;
  onChange: (docs: string) => void;
}) {
  return (
    <Suspense
      fallback={<div className="h-full w-full animate-pulse bg-muted/30" aria-busy="true" />}
    >
      <DocsEditorImpl value={value} onChange={onChange} />
    </Suspense>
  );
}
