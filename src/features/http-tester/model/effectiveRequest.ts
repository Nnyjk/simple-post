import type { AuthConfig, EndpointDetail, KeyValue, RequestBody } from '@/types/domain';
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
  };
}
