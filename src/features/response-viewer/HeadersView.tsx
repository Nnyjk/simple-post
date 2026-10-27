/** 响应头的键值表。 */
export function HeadersView({ headers }: { headers: Record<string, string> }) {
  return (
    <div className="p-4">
      <table className="w-full text-sm">
        <tbody>
          {Object.entries(headers).map(([k, v]) => (
            <tr key={k} className="border-b border-border/50 last:border-0">
              <td className="w-1/3 py-1.5 pr-4 align-top font-mono text-xs text-muted-foreground">
                {k}
              </td>
              <td className="py-1.5 align-top font-mono text-xs text-foreground/90 break-all">
                {v}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
