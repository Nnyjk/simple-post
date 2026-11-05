/**
 * 判断一个监听地址是不是本机回环地址。
 *
 * 「回环」这件事在三个地方要判：设置面板那条「远程访问开关目前不参与判断」的提示、
 * 设置面板的端点地址计算、以及 MCP 端点地址的计算。故它单独成文件，三处共用一份判断——
 * 三处各写一遍时，其中一处改了另外两处不会跟着改，而那正是「地址算错了但没人报错」
 * 的来源。
 *
 * 与 Rust 侧 `core::domain::is_loopback_host` 一一对应。
 */
export function isLoopbackHost(host: string): boolean {
  const value = host.trim().toLowerCase();
  return value === '127.0.0.1' || value === '::1' || value === 'localhost';
}
