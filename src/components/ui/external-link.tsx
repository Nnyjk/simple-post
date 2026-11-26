import { type ReactNode } from 'react';
import { COMMANDS, IS_TAURI_SHELL, invokeCmd } from '@/lib/tauri-commands';

interface ExternalLinkProps {
  href: string;
  /** 读屏与悬停提示共用的一句话；图标链接没有可见文字，必须给。 */
  label: string;
  className?: string;
  children: ReactNode;
}

/**
 * 指向站外网页的链接。
 *
 * 浏览器形态就是一条普通的 `target="_blank"` 锚点。桌面壳里 webview 会拒绝新建窗口
 * （wry 的 `NewWindowRequested` 没有处理器时一律置为已处理），故那一侧改为
 * `open_external_url`，由操作系统用默认浏览器打开同一条地址。
 */
export function ExternalLink({ href, label, className, children }: ExternalLinkProps) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      title={label}
      aria-label={label}
      className={className}
      onClick={(e) => {
        if (!IS_TAURI_SHELL) return;
        // 桌面壳里锚点本身打不开，交给 Rust；浏览器形态继续走默认行为。
        e.preventDefault();
        void invokeCmd(COMMANDS.openExternalUrl, { url: href }).catch((error) => {
          console.warn('[external-link] 打开浏览器失败', error);
        });
      }}
    >
      {children}
    </a>
  );
}
