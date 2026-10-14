/**
 * 启动闪屏闸 —— 拆掉 `index.html` 里的 HTML 闪屏。
 *
 * 闪屏刻意是静态标记而非 React 组件：冷启动里最长的一个阶段是 entry chunk 的下载与执行，
 * 而这期间 JS 渲染出的*任何东西*都不可见；用 HTML 上色能白得覆盖这段窗口。
 *
 * 于是只剩交接问题。React 一挂载就移除闪屏，只是把空白屏挪后几百毫秒 —— 那时 store 仍没有
 * 数据。故本闸等到应用真能显示点什么的第一个时刻：`loaded`（数据到了）或
 * `bootstrapError`（永远不会有，而横幅必须可读）。
 *
 * 由 `App.tsx` 挂载；它不渲染任何东西。
 */
import { useEffect } from 'react';
import { useAppStore } from '@/stores/app-store';

export function BootSplashGate() {
  const loaded = useAppStore((s) => s.loaded);
  const bootstrapError = useAppStore((s) => s.bootstrapError);

  const done = loaded || bootstrapError !== null;

  useEffect(() => {
    if (!done) return;
    // 每次调用都用 `getElementById` 而非预先捕获的 ref：该节点归 index.html 所有，惰性
    // 查找使这段代码在渲染与 effect 之间文档被换掉时依然正确。
    document.getElementById('boot-splash')?.remove();
  }, [done]);

  return null;
}
