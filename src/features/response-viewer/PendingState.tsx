/** 请求进行中的占位。 */
export function PendingState() {
  return (
    <section className="flex h-full min-h-0 flex-col items-center justify-center border-t border-border bg-card/30">
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <span className="h-2 w-2 animate-pulse rounded-full bg-primary" />
        <span>请求中…</span>
      </div>
    </section>
  );
}
