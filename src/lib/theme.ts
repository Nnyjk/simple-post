import { useEffect, useState } from 'react';
import { useAppStore } from '@/stores/app-store';

/**
 * 应用当前是否渲染在暗色模式。
 *
 * 把三态偏好（`dark` | `light` | `system`）在需要时连同 OS 偏好一起收敛成一个
 * 布尔量。用于需要替换硬编码主题属性的组件（如 CodeMirror）；普通样式请用 Tailwind
 * 的 `dark:` 变体——它已经跟着 `<html>` 上的 `.dark` 类走。
 *
 * 单独一个模块（而非放进 `utils.ts`）是因为这里的 `useAppStore` import 会闭合
 * `app-store → crud-slice → utils → app-store` 这个运行时循环依赖，并在 store 测试里
 * 破坏 TDZ 初始化。让本 hook 不出现在 slice 传递依赖的 `utils.ts` import 面上，store
 * 的加载顺序才成立。
 */
export function useIsDark(): boolean {
  const theme = useAppStore((s) => s.theme);
  const [systemDark, setSystemDark] = useState(
    () =>
      typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches,
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
