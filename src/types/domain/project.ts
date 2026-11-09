export interface Project {
  id: string;
  name: string;
  description?: string | null;
  color: string;
  createdAt: number;
  updatedAt: number;
}

/**
 * 路径前缀改写规则：把端点**自己 URL** 的路径前缀从 `from` 换成 `to`。
 *
 * - `from`（起点）是端点自己 URL 的路径前缀，如 `/api/air`。它**按原文匹配**：不做
 *   `{{var}}` 替换；模块地址自带的路径段也不参与匹配（模块地址与端点地址互不干扰）。
 * - `to`（目标）是替换后的路径前缀，如 `/api/project-a/air`。空串表示把这段前缀整个去掉。
 *
 * 命中要求落在路径边界上：URL 以 `from` 开头，且其后紧跟 `/`、`?` 或 `#`（或 URL 恰好
 * 等于 `from`）。没有这一条，`/api/air` 会命中 `/api/airplane`。
 */
export interface RewriteRule {
  from: string;
  to: string;
}

/**
 * 一个（环境 × 模块）格子里的地址配置。两个字段回答两个问题：`url` 是**模块地址**
 * （文档里写的、模块对外声明的那个地址），`rewrite` 是**路径前缀改写规则**（只改写端点
 * 自己的 URL 路径）。
 *
 * 两者互不干扰：模块地址里的主机与路径段一概不动。故
 * `url = "127.0.0.1:8080"`、`rewrite = { from: "/api/air", to: "/api/project-a/air" }` 与端点
 * 路径 `/api/air/xxxxx` 合成的实际请求是 `127.0.0.1:8080/api/project-a/air/xxxxx`。
 *
 * `rewrite` 缺席 = 没有改写，端点自己的 URL 原样使用。
 */
export interface ModuleBaseUrl {
  url: string;
  rewrite?: RewriteRule | null;
}

export interface Environment {
  id: string;
  projectId: string;
  name: string;
  /**
   * 唯一的 URL 层：`moduleId → ModuleBaseUrl`。
   *
   * 每个格子都是独立值，彼此**不继承也不回退**——缺项或地址为空即「该组合没有值」，不会借用
   * 别的模块的格子。`resolveEndpointBaseUrl` 读 `moduleBaseUrls[moduleId]`。
   */
  moduleBaseUrls: Record<string, ModuleBaseUrl>;
  /** 自由模板变量（如 `token`、`apiKey`）。baseUrl 值在 `moduleBaseUrls`，不在这里。 */
  variables: Record<string, string>;
  /**
   * 被停用的变量名。**照样存、照样显示，但不参与替换。**
   *
   * 与 `variables` 平行，而不是把值改成 `{value, enabled}`：后者会改掉
   * `environments.variables_json` 列的既有格式，写入这一列之前的行读不回来。平行集合是纯
   * 增量：读回来是空集合，语义等价于「全部启用」。
   */
  disabledVariables: string[];
  /**
   * 创建时刻（unix 毫秒）。「默认环境」按它认定：**一个项目里最早创建的那个环境**是那个项目
   * 的默认环境。默认环境是项目的属性——谁发请求时没点名环境，就用它。
   *
   * 这里**没有**「当前环境」这一列。当前环境是每个客户端自己的实时状态（桌面窗口、网页标签各
   * 一份，各切各的），落库会让 web 端多个人互相踩。
   */
  createdAt: number;
}
