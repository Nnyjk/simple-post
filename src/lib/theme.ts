import { useEffect, useState } from 'react';
import { useAppStore } from '@/stores/app-store';

/**
 * Returns true when the app is currently rendering in dark mode.
 *
 * Resolves the three-mode preference (`dark` | `light` | `system`) into a
 * single boolean by also tracking the OS preference when needed. Use this
 * inside components that need to swap a hard-coded theme prop (e.g. CodeMirror)
 * — for plain styling, prefer the Tailwind `dark:` variant which already
 * follows the `.dark` class on `<html>`.
 *
 * Lives in its own module (instead of `utils.ts`) because the
 * `useAppStore` import closes a runtime circular dependency
 * `app-store → crud-slice → utils → app-store` that breaks TDZ
 * initialization in store tests. Keeping this hook off the
 * `utils.ts` import surface that slices transitively depend on
 * preserves the store's load order.
 */
export function useIsDark(): boolean {
  const theme = useAppStore((s) => s.theme);
  const [systemDark, setSystemDark] = useState(() =>
    typeof window !== 'undefined' &&
    window.matchMedia('(prefers-color-scheme: dark)').matches,
  );

  useEffect(() => {
    if (theme !== 'system') return;
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = () => setSystemDark(mq.matches);
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, [theme]);

  if (theme === 'dark') return true;
  if (theme === 'light') return false;
  return systemDark;
}