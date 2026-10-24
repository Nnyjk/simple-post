/**
 * URL 输入框里的 env 变量选择器：候选条目、键盘导航与插入。
 *
 * 它是纯展示 + 键盘交互，选择器条目由「当前环境 + 最后一个 `{{` 之后输入的片段」
 * 派生；真正的写回由调用点给的 `onUrlChange` 完成，故本 hook 不碰 store。
 */
import { useMemo, useState, type KeyboardEvent, type RefObject } from 'react';
import { extractPartialVar, shouldShowVarPicker, type VarEntry } from './EnvVarPicker';
import type { Environment } from '@/types/domain';

/**
 * 返回选择器的状态与三个处理器（改 URL / 插入变量 / 键盘导航）。
 *
 * `urlInputRef` 用于插入后把焦点与光标放回输入框。调用点必须在任何提前返回之上
 * ——它们是 hook。
 */
export function useUrlVarPicker({
  endpointEnv,
  dynamicEntries,
  liveUrl,
  urlInputRef,
  onUrlChange,
}: {
  endpointEnv: Environment | undefined;
  /**
   * 动态变量的候选条目。它们与环境变量并进同一份列表——不新开页签。
   *
   * 由调用点给（它读 store），故本 hook 仍不碰 store。
   */
  dynamicEntries?: VarEntry[];
  liveUrl: string;
  urlInputRef: RefObject<HTMLInputElement | null>;
  /** 把新 URL 写回草稿；返回是否真的写了（没有接口可写时为 false）。 */
  onUrlChange: (next: string) => boolean;
}) {
  const [varPickerOpen, setVarPickerOpen] = useState(false);
  const [varSelected, setVarSelected] = useState(0);

  // env 变量选择器的条目：接口自己的环境（即本接口解析 `{{x}}` 所依据的那份变量）
  // + 用户在最后一个 `{{` 之后输入的片段。
  const allEnvEntries = useMemo(() => {
    const env = endpointEnv
      ? Object.entries(endpointEnv.variables).map(([k, v]) => ({
          key: k,
          value: v,
          origin: 'environment' as const,
        }))
      : [];
    const dynamic = dynamicEntries ?? [];
    // 同名时动态变量排前面：它在替换时压过环境变量（见 `mergePool`），
    // 故列表里的先后顺序要与替换时的优先级一致。
    return [...dynamic, ...env].sort((a, b) => a.key.localeCompare(b.key));
  }, [endpointEnv, dynamicEntries]);
  const partialVar = extractPartialVar(liveUrl);
  const filteredEntries = useMemo(() => {
    const q = (partialVar ?? '').toLowerCase();
    if (!q) return allEnvEntries;
    return allEnvEntries.filter((e) => (e.key ?? '').toLowerCase().includes(q));
  }, [allEnvEntries, partialVar]);

  const handleUrlChange = (next: string) => {
    if (!onUrlChange(next)) return;
    setVarPickerOpen(shouldShowVarPicker(next));
    if (!shouldShowVarPicker(next)) setVarSelected(0);
  };

  const insertVar = (key: string) => {
    // 把最后一个 `{{` 及其后的变量名片段（截至 `}}`、空白或又一个 `{`）替换为选中的
    // 键，并补上闭合大括号。
    const base = liveUrl;
    const next = base.replace(/\{\{[^}\s]*$/, `{{${key}}}`);
    if (!onUrlChange(next)) return;
    setVarPickerOpen(false);
    setVarSelected(0);
    // 恢复焦点并把光标移到插入 token 的末尾。
    window.setTimeout(() => {
      const el = urlInputRef.current;
      if (!el) return;
      el.focus();
      const caret = next.length;
      try {
        el.setSelectionRange(caret, caret);
      } catch {
        /* 数字输入框之类 —— type=text 用不到 */
      }
    }, 0);
  };

  // 自定义 把 URL 栏变成完整地址，故把光标放过去并全选旧内容 —— 直接输入即替换旧
  // path 而非追加。延后一拍执行，使其发生在切换模式的重渲染（以及 popover 关闭）之后。
  const focusUrlForFullAddress = () => {
    window.setTimeout(() => {
      const el = urlInputRef.current;
      if (!el) return;
      el.focus();
      try {
        el.select();
      } catch {
        /* 不是文本输入框 —— 无内容可全选 */
      }
    }, 0);
  };

  const handleUrlKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (!varPickerOpen) return;
    // 放过带修饰键的组合，全局快捷键（Ctrl+Enter、Ctrl+S）才仍能触发。
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (filteredEntries.length > 0) {
        setVarSelected((s) => (s + 1) % filteredEntries.length);
      }
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (filteredEntries.length > 0) {
        setVarSelected((s) => (s - 1 + filteredEntries.length) % filteredEntries.length);
      }
    } else if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      const entry = filteredEntries[varSelected] ?? filteredEntries[0];
      if (entry) insertVar(entry.key);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      setVarPickerOpen(false);
    }
  };

  return {
    varPickerOpen,
    varSelected,
    filteredEntries,
    setVarSelected,
    setVarPickerOpen,
    handleUrlChange,
    handleUrlKeyDown,
    insertVar,
    focusUrlForFullAddress,
  };
}
