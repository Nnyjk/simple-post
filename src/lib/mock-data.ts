import type {
  Project,
  Module,
  Collection,
  Endpoint,
  Environment,
} from '@/types/domain';

const now = Date.now();
const day = 24 * 60 * 60 * 1000;

export const mockProjects: Project[] = [
  {
    id: 'p_demo',
    name: 'Demo API',
    description: '示例项目 — 用户与订单',
    // 1 default baseUrl def (preserved from the legacy single-URL
    // field). The store migration in `app-store.ts` pushes the old
    // `baseUrl` string into `env.baseUrls[def_default]`, so existing
    // env data still produces a valid URL.
    baseUrlDefinitions: [
      { id: 'def_default', name: '默认' },
    ],
    defaultBaseUrlDefinitionId: 'def_default',
    baseUrl: 'https://api.demo.dev',
    color: '#60a5fa',
    createdAt: now - 30 * day,
    updatedAt: now - 1 * day,
  },
  {
    id: 'p_invoice',
    name: '发票服务',
    description: '开票、抬头、PDF 生成',
    baseUrlDefinitions: [
      { id: 'def_default', name: '默认' },
    ],
    defaultBaseUrlDefinitionId: 'def_default',
    baseUrl: 'https://invoice.example.com',
    color: '#a855f7',
    createdAt: now - 14 * day,
    updatedAt: now - 2 * day,
  },
  {
    id: 'p_iot',
    name: 'IoT 设备',
    description: '设备影子 / 指令下发',
    baseUrlDefinitions: [
      { id: 'def_default', name: '默认' },
    ],
    defaultBaseUrlDefinitionId: 'def_default',
    baseUrl: 'https://iot.internal/api/v2',
    color: '#f97316',
    createdAt: now - 60 * day,
    updatedAt: now - 7 * day,
  },
];

export const mockModules: Module[] = [
  // Demo API
  { id: 'm_auth', projectId: 'p_demo', name: '用户中心', description: '认证、用户画像', sortOrder: 0, expanded: true },
  { id: 'm_order', projectId: 'p_demo', name: '订单', description: '订单生命周期', sortOrder: 1, expanded: true },
  { id: 'm_pay', projectId: 'p_demo', name: '支付', description: '收银台、对账', sortOrder: 2 },
  // 发票
  { id: 'm_title', projectId: 'p_invoice', name: '抬头管理', sortOrder: 0 },
  { id: 'm_issue', projectId: 'p_invoice', name: '开票', sortOrder: 1 },
  // IoT
  { id: 'm_shadow', projectId: 'p_iot', name: '设备影子', sortOrder: 0 },
];

export const mockCollections: Collection[] = [
  // auth
  { id: 'c_login', moduleId: 'm_auth', parentCollectionId: null, name: '登录注册', sortOrder: 0, expanded: true },
  { id: 'c_profile', moduleId: 'm_auth', parentCollectionId: null, name: '用户资料', sortOrder: 1 },
  // order
  { id: 'c_order_crud', moduleId: 'm_order', parentCollectionId: null, name: '订单 CRUD', sortOrder: 0, expanded: true },
  { id: 'c_order_status', moduleId: 'm_order', parentCollectionId: null, name: '状态流转', sortOrder: 1 },
  // pay
  { id: 'c_cashier', moduleId: 'm_pay', parentCollectionId: null, name: '收银台', sortOrder: 0 },
  // 发票
  { id: 'c_title_crud', moduleId: 'm_title', parentCollectionId: null, name: '抬头 CRUD', sortOrder: 0 },
  { id: 'c_issue_flow', moduleId: 'm_issue', parentCollectionId: null, name: '开票流程', sortOrder: 0 },
  // IoT
  { id: 'c_shadow_op', moduleId: 'm_shadow', parentCollectionId: null, name: '影子读写', sortOrder: 0 },
];

export const mockEndpoints: Endpoint[] = [
  {
    id: 'e_login_sms',
    collectionId: 'c_login',
    name: '短信登录',
    description: '通过短信验证码登录',
    method: 'POST',
    url: '/auth/login/sms',
    params: [],
    headers: [
      { id: 'k1', key: 'Content-Type', value: 'application/json', enabled: true },
    ],
    body: {
      mode: 'json',
      content: JSON.stringify(
        {
          phone: '13800138000',
          code: '654321',
          deviceId: 'web-{{env}}',
        },
        null,
        2,
      ),
    },
    auth: { type: 'none' },
    docs:
      '## 短信登录\n\n使用手机号 + 短信验证码完成登录。\n\n- 验证码 5 分钟内有效\n- 同一手机号 1 分钟内只能发 1 条',
    tags: ['登录', '公开'],
    sortOrder: 0,
    createdAt: now - 25 * day,
    updatedAt: now - 1 * day,
  },
  {
    id: 'e_login_pwd',
    collectionId: 'c_login',
    name: '账号密码登录',
    method: 'POST',
    url: '/auth/login',
    params: [],
    headers: [],
    body: {
      mode: 'json',
      content: JSON.stringify({ username: '', password: '' }, null, 2),
    },
    auth: { type: 'none' },
    tags: ['登录'],
    sortOrder: 1,
    createdAt: now - 25 * day,
    updatedAt: now - 5 * day,
  },
  {
    id: 'e_user_get',
    collectionId: 'c_profile',
    name: '获取用户信息',
    method: 'GET',
    url: '/users/{{userId}}',
    params: [
      { id: 'p1', key: 'include', value: 'profile,permissions', enabled: true, description: '附加字段' },
    ],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { type: 'bearer', bearer: '{{token}}' },
    tags: ['用户'],
    sortOrder: 0,
    createdAt: now - 20 * day,
    updatedAt: now - 2 * day,
  },
  {
    id: 'e_user_update',
    collectionId: 'c_profile',
    name: '更新用户资料',
    method: 'PATCH',
    url: '/users/{{userId}}',
    params: [],
    headers: [],
    body: { mode: 'json', content: JSON.stringify({ nickname: '', avatar: '' }, null, 2) },
    auth: { type: 'bearer', bearer: '{{token}}' },
    tags: ['用户'],
    sortOrder: 1,
    createdAt: now - 20 * day,
    updatedAt: now - 3 * day,
  },
  {
    id: 'e_order_list',
    collectionId: 'c_order_crud',
    name: '订单列表',
    method: 'GET',
    url: '/orders',
    params: [
      { id: 'p1', key: 'page', value: '1', enabled: true },
      { id: 'p2', key: 'size', value: '20', enabled: true },
      { id: 'p3', key: 'status', value: 'PAID', enabled: false, description: 'PAID/SHIPPED/DONE' },
    ],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { type: 'bearer', bearer: '{{token}}' },
    docs: '## 订单列表\n分页参数 `page` 从 1 开始。',
    tags: ['订单', '分页'],
    sortOrder: 0,
    createdAt: now - 15 * day,
    updatedAt: now - 1 * day,
  },
  {
    id: 'e_order_create',
    collectionId: 'c_order_crud',
    name: '创建订单',
    method: 'POST',
    url: '/orders',
    params: [],
    headers: [],
    body: {
      mode: 'json',
      content: JSON.stringify(
        {
          skuId: 'sku_123',
          quantity: 1,
          addressId: 'addr_001',
          couponId: null,
        },
        null,
        2,
      ),
    },
    auth: { type: 'bearer', bearer: '{{token}}' },
    tags: ['订单'],
    sortOrder: 1,
    createdAt: now - 15 * day,
    updatedAt: now - 2 * day,
  },
  {
    id: 'e_order_cancel',
    collectionId: 'c_order_status',
    name: '取消订单',
    method: 'DELETE',
    url: '/orders/{{orderId}}',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { type: 'bearer', bearer: '{{token}}' },
    tags: ['订单'],
    sortOrder: 0,
    createdAt: now - 10 * day,
    updatedAt: now - 1 * day,
  },
  {
    id: 'e_cashier_create',
    collectionId: 'c_cashier',
    name: '创建收银台',
    method: 'POST',
    url: '/cashier',
    params: [],
    headers: [],
    body: { mode: 'json', content: JSON.stringify({ orderId: '', channel: 'WECHAT' }, null, 2) },
    auth: { type: 'bearer', bearer: '{{token}}' },
    tags: ['支付'],
    sortOrder: 0,
    createdAt: now - 8 * day,
    updatedAt: now - 1 * day,
  },
  {
    id: 'e_title_list',
    collectionId: 'c_title_crud',
    name: '抬头列表',
    method: 'GET',
    url: '/invoice/titles',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { type: 'bearer', bearer: '{{token}}' },
    tags: [],
    sortOrder: 0,
    createdAt: now - 6 * day,
    updatedAt: now - 1 * day,
  },
  {
    id: 'e_issue_apply',
    collectionId: 'c_issue_flow',
    name: '申请开票',
    method: 'POST',
    url: '/invoice/issue',
    params: [],
    headers: [],
    body: { mode: 'json', content: JSON.stringify({ orderId: '', titleId: '', email: '' }, null, 2) },
    auth: { type: 'bearer', bearer: '{{token}}' },
    tags: ['发票'],
    sortOrder: 0,
    createdAt: now - 5 * day,
    updatedAt: now - 1 * day,
  },
  {
    id: 'e_shadow_get',
    collectionId: 'c_shadow_op',
    name: '读取设备影子',
    method: 'GET',
    url: '/shadow/{{deviceId}}',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { type: 'apikey', apikey: { key: 'X-API-Key', value: '{{apiKey}}', in_: 'header' } },
    tags: ['IoT'],
    sortOrder: 0,
    createdAt: now - 4 * day,
    updatedAt: now - 1 * day,
  },
  {
    id: 'e_shadow_update',
    collectionId: 'c_shadow_op',
    name: '更新期望状态',
    method: 'PUT',
    url: '/shadow/{{deviceId}}/desired',
    params: [],
    headers: [],
    body: { mode: 'json', content: JSON.stringify({ switch: true, brightness: 80 }, null, 2) },
    auth: { type: 'apikey', apikey: { key: 'X-API-Key', value: '{{apiKey}}', in_: 'header' } },
    tags: ['IoT'],
    sortOrder: 1,
    createdAt: now - 4 * day,
    updatedAt: now - 1 * day,
  },
];

export const mockEnvironments: Environment[] = [
  {
    id: 'env_dev',
    projectId: 'p_demo',
    name: 'dev',
    // baseUrls is keyed by BaseUrlDefinition.id. The single-key
    // shape here matches the single default def on the project —
    // enough for the new picker's "项目默认 → env: dev" flow to
    // resolve to https://api.demo.dev.
    baseUrls: {
      def_default: 'https://api.demo.dev',
    },
    variables: {
      token: 'eyJhbGciOiJIUzI1NiJ9.dev',
      env: 'dev',
    },
    isActive: true,
  },
  {
    id: 'env_staging',
    projectId: 'p_demo',
    name: 'staging',
    baseUrls: {
      def_default: 'https://api.staging.demo',
    },
    variables: {
      token: 'eyJhbGciOiJIUzI1NiJ9.staging',
      env: 'staging',
    },
    isActive: false,
  },
  {
    id: 'env_prod',
    projectId: 'p_demo',
    name: 'prod',
    baseUrls: {
      def_default: 'https://api.demo.com',
    },
    variables: {
      token: 'eyJhbGciOiJIUzI1NiJ9.prod',
      env: 'prod',
    },
    isActive: false,
  },
];
