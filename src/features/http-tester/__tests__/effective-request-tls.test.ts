/**
 * 端点自己那份 HTTPS 出站设置怎么与已存的那一行合并。
 *
 * 这一层决定「这个接口用哪一份证书」，故它的三态必须逐个钉住：
 *
 *   1. 草稿没碰过这一项 → 用已存的那份（没碰过 = 不动它）。
 *   2. 草稿写了一份设置 → 用草稿那份（草稿是用户此刻在编辑的那份）。
 *   3. 草稿显式写 `null` → 回到继承全局，**压过**已存的那份。用 `??` 回落会把
 *      这一态读成已存的那份设置，于是「关掉继承全局」这个动作在界面上看得见、
 *      在发送时却没有发生。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { buildEffectiveRequest } from '../model/effectiveRequest';
import type { EndpointDetail, TlsSettings } from '@/types/domain';

const OWN: TlsSettings = {
  skipVerify: true,
  caCertificatePath: 'C:\\certs\\ca.pem',
  clientCertificatePath: null,
  clientKeyPath: null,
};

/** 一行已存的请求配置，`tls` 可按用例替换。 */
function detail(tls?: TlsSettings | null): EndpointDetail {
  return {
    nodeId: 'ep-1',
    method: 'GET',
    url: '/x',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    tags: [],
    ...(tls !== undefined ? { tls } : {}),
  };
}

describe('端点级 HTTPS 设置：草稿与已存那一行的合并', () => {
  it('草稿没碰过这一项时用已存的那份', () => {
    const effective = buildEffectiveRequest({}, detail(OWN));
    assert.deepEqual(effective.tls, OWN);
  });

  it('已存的那一行没有这一项、也没人编辑过时就是继承全局', () => {
    const effective = buildEffectiveRequest(undefined, detail());
    assert.equal(effective.tls, null, 'null = 继承全局默认');
  });

  it('草稿写了自己的那一份就以草稿为准', () => {
    const effective = buildEffectiveRequest({ tls: OWN }, detail(null));
    assert.deepEqual(effective.tls, OWN);
  });

  it('草稿显式写 null 时压过已存的那一份', () => {
    const effective = buildEffectiveRequest({ tls: null }, detail(OWN));
    assert.equal(effective.tls, null, '回到继承全局，不能读成已存的那份');
  });

  it('草稿里没有 tls 这个键时不算「写过」', () => {
    const effective = buildEffectiveRequest({ url: '/y' }, detail(OWN));
    assert.deepEqual(effective.tls, OWN);
  });
});
