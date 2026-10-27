/**
 * 「我们将要发出什么」的唯一构造核：真实发送（`src/lib/http.ts`）与
 * copy-as-cURL 预览都从这里派生，两边因此不可能对同一请求有不同描述。
 *
 *   prepareRequest()   → 唯一的组装处：请求 → URL + 有序 header + body
 *                        （变量未解析），别处不再拼装请求
 *   buildFetchInit()   → prepareRequest 的 fetch 适配器
 *   buildCurlCommand() → prepareRequest 的 shell 行适配器
 *
 * 纯函数，无 I/O。I/O 归运行时的原生 `fetch`，本模块只把用户可编辑的
 * 请求形状转成运行时要的 URL + init（或 cURL 字符串）。
 *
 * 实现分布在 `http-build/` 下按职责分文件，本文件只做重导出，
 * 调用方的 import 路径不变。
 */
export {
  type BuildInput,
  type BuiltFetch,
  type PrepareRequestInput,
  type PreparedRequest,
} from './http-build/types';
export { prepareRequest } from './http-build/prepare';
export { buildFetchInit } from './http-build/fetch';
export { buildCurlCommand, preparedToCurl } from './http-build/curl';
