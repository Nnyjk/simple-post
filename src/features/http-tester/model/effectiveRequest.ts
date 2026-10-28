import type {
  AuthConfig,
  EndpointDetail,
  KeyValue,
  RequestBody,
  TlsSettings,
} from '@/types/domain';
import type { EndpointDraft } from '@/stores/store-types';

/**
 * 缺失 `endpoint_details` 行时的占位，让 effective 请求有类型可读。
 * 按引用共享：所有编辑器都是先展开再写，不会就地修改。
 */
export const EMPTY_BODY: RequestBody = { mode: 'none', content: '' };
export const EMPTY_AUTH: AuthConfig = { kind: 'none' };

/**
 * 当前生效的请求配置：已存的 detail 叠上草稿。
 *
 * 所有请求配置编辑器都写草稿而非该行，故这里是唯一决定「用户所见即为何」的地方 ——
 * 发送、保存、cURL 预览与校验都读它。URL / method 不放进本对象：URL 栏还要原始值做
 * 自己的选择器与光标逻辑。
 */
export interface EffectiveRequest {
  params: KeyValue[];
  pathParams: KeyValue[];
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
  docs: string;
  tags: string[];
  preRequestScript: string;
  testScript: string;
  /**
   * 该端点自己的 HTTPS 出站设置。`null` = 继承全局默认（`RequestSettings.tls`）。
   *
   * 与其它字段不同，它有第三态：草稿里显式写过 `null` 表示「回到继承全局」，那必须
   * 压过已存的那份，故这里不能用 `??` 回落。
   */
  tls: TlsSettings | null;
}

/** 草稿优先，回落到已存的那份。 */
export function buildEffectiveRequest(
  draft: EndpointDraft | undefined,
  detail: EndpointDetail | undefined,
): EffectiveRequest {
  return {
    params: draft?.params ?? detail?.params ?? [],
    pathParams: draft?.pathParams ?? detail?.pathParams ?? [],
    headers: draft?.headers ?? detail?.headers ?? [],
    body: draft?.body ?? detail?.body ?? EMPTY_BODY,
    auth: draft?.auth ?? detail?.auth ?? EMPTY_AUTH,
    docs: draft?.docs ?? detail?.docs ?? '',
    tags: draft?.tags ?? detail?.tags ?? [],
    preRequestScript: draft?.preRequestScript ?? detail?.preRequestScript ?? '',
    testScript: draft?.testScript ?? detail?.testScript ?? '',
    // 草稿显式写过 `tls`（含显式写 `null`）就以草稿为准；没写过才用已存的那份。
    // 用 `??` 回落会把「草稿写 null = 回到继承全局」读成已存的那份设置。
    tls: draft?.tls !== undefined ? draft.tls : (detail?.tls ?? null),
  };
}
