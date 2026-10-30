export interface Project {
  id: string;
  name: string;
  description?: string | null;
  color: string;
  createdAt: number;
  updatedAt: number;
}

/**
 * 一个（环境 × 模块）格子里的地址配置。
 *
 * 两个字段回答两个问题：
 *
 *   - `url` 是**后端 api 地址** —— 文档里写的、模块对外声明的那个地址。
 *   - `proxy` 是**实际访问地址** —— 接口经代理网关转代后真正要打的那个地址。
 *
 * 两者不一致时，请求发出前用 `proxy` 替换 `url` 这个前缀，其余部分原样保留。故
 * `url = "http://svc.internal:8080/api/a"`、`proxy = "https://gw.corp.com/api/xxxx/a"`
 * 与端点路径 `/orders/list` 合成的实际请求是
 * `https://gw.corp.com/api/xxxx/a/orders/list`。
 *
 * `proxy` 缺席 = 没有代理，请求直接打 `url`。
 */
export interface ModuleBaseUrl {
  url: string;
  proxy?: string | null;
}

export interface Environment {
  id: string;
  projectId: string;
  name: string;
  /**
   * 唯一的 URL 层：`moduleId → ModuleBaseUrl`。
   *
   * 每个格子都是独立值，彼此**不继承也不回退** —— 缺项或地址为空即「该组合没有值」，
   * 不会借用别的模块的格子。`resolveEndpointBaseUrl` 读 `moduleBaseUrls[moduleId]`。
   */
  moduleBaseUrls: Record<string, ModuleBaseUrl>;
  /** 自由模板变量（如 `token`、`apiKey`）。baseUrl 值在 `moduleBaseUrls`，不在这里。 */
  variables: Record<string, string>;
  /**
   * 被停用的变量名。**照样存、照样显示，但不参与替换。**
   *
   * 与 `variables` 平行，而不是把值改成 `{value, enabled}`：后者会改掉
   * `environments.variables_json` 列的既有格式，旧行读不回来。平行集合是纯增量：
   * 旧行读回来是空集合，语义等价于「全部启用」。
   */
  disabledVariables: string[];
  isActive: boolean;
}
