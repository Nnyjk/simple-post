/**
 * `sendRequest` 的集成测试（认证与 URL 模板）：认证派生出的 `Authorization` /
 * query apikey 到达服务端，且 URL 里的 `{{var}}` 对当前环境解析。
 *
 * 其余请求形状见 `http-request-shape.test.ts`，响应处理见
 * `http-response-transport.test.ts`。服务端脚手架在 `http-server-harness.ts`。
 */
import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import type { AuthConfig } from '../../types/domain';

import {
  captured,
  getBaseUrl,
  lastRequest,
  send,
  startHarness,
  stopHarness,
  ENV,
} from './http-server-harness';

before(startHarness);
after(stopHarness);
// ============================================================
// 5. 认证
// ============================================================

describe('sendRequest: auth', () => {
  it('Bearer: Authorization header reaches the server', async () => {
    captured.length = 0;
    await send({ auth: { kind: 'bearer', bearer: 'eyJ.tok' } as AuthConfig });
    assert.equal(lastRequest().headers['authorization'], 'Bearer eyJ.tok');
  });

  it('Bearer: resolves {{token}} from env', async () => {
    captured.length = 0;
    await send({ auth: { kind: 'bearer', bearer: '{{token}}' } as AuthConfig, env: ENV });
    assert.equal(lastRequest().headers['authorization'], 'Bearer eyJ.tok');
  });

  it('Basic: Authorization header is base64(user:pass)', async () => {
    captured.length = 0;
    await send({
      auth: { kind: 'basic', basic: { username: 'alice', password: 'secret' } } as AuthConfig,
    });
    const hdr = lastRequest().headers['authorization'];
    assert.ok(hdr?.startsWith('Basic '));
    const decoded = Buffer.from(hdr!.slice('Basic '.length), 'base64').toString('utf-8');
    assert.equal(decoded, 'alice:secret');
  });

  it('API Key (header) reaches the server as <key>: <value>', async () => {
    captured.length = 0;
    await send({
      auth: {
        kind: 'apikey',
        apikey: { key: 'X-API-Key', value: 'k_123', inLocation: 'header' },
      } as AuthConfig,
    });
    assert.equal(lastRequest().headers['x-api-key'], 'k_123');
  });

  it('API Key (query) is appended to the URL', async () => {
    captured.length = 0;
    await send({
      auth: {
        kind: 'apikey',
        apikey: { key: 'apiKey', value: 'k_123', inLocation: 'query' },
      } as AuthConfig,
    });
    assert.equal(lastRequest().url, '/api/x?apiKey=k_123');
    assert.equal(lastRequest().headers['x-api-key'], undefined);
    assert.equal(lastRequest().headers['apikey'], undefined);
  });

  it('API Key (header) with {{apiKey}} resolves from env', async () => {
    captured.length = 0;
    await send({
      auth: {
        kind: 'apikey',
        apikey: { key: 'X-API-Key', value: '{{apiKey}}', inLocation: 'header' },
      } as AuthConfig,
      env: ENV,
    });
    assert.equal(lastRequest().headers['x-api-key'], 'k_123');
  });
});

// ============================================================
// 6. URL 里的 {{var}} 解析
// ============================================================

describe('sendRequest: URL template resolution', () => {
  it('resolves {{userId}} in URL against env', async () => {
    captured.length = 0;
    await send({ url: `${getBaseUrl()}/api/users/{{userId}}`, env: ENV });
    assert.equal(lastRequest().url, '/api/users/u_demo');
  });
});
