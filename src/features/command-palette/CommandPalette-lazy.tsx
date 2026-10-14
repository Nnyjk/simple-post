/**
 * 命令面板（Ctrl+K）的 lazy 边界。
 *
 * 面板被 `TopBar` 无条件挂载，却只对一次按键才有反应，故把它留在 entry 图里意味着每次冷
 * 启动都要为一个几乎没人一打开就会用的对话框买单。
 *
 * `TopBar` 用 `Suspense` 且 fallback 为 `null` 渲染它：在 `open` 翻成 true 之前没什么可
 * 显示，而那时用户已经按过键，chunk 通常已在内存或只差一次取件。
 */
import { lazy } from 'react';

const CommandPaletteImpl = lazy(() =>
  import('./CommandPalette').then((m) => ({ default: m.CommandPalette })),
);

export function CommandPalette(props: { open: boolean; onClose: () => void }) {
  return <CommandPaletteImpl {...props} />;
}
