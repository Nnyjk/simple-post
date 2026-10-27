import type { BaseUrlDefinition } from './tree';

export interface Project {
  id: string;
  name: string;
  description?: string;
  /** @deprecated 改用 `baseUrlDefinitions` + `Environment.moduleBaseUrls`；仅为兼容保留，新代码不要引用。 */
  baseUrl?: string;
  /**
   * 本项目对外暴露的具名 baseUrl 槽位。**一份列表，全模块共享**——
   * 设置页表格是这些服务与本项目模块的笛卡尔积，HttpTester 选择器也列出全部。
   * 新建项目此处为空。
   */
  baseUrlDefinitions: BaseUrlDefinition[];
  /**
   * 项目级默认服务（★）——endpoint 未指定服务时按它解析，即
   * `baseUrlDefinitionId === null`。`null` 是有意状态：有服务但用户没打星，
   * 未指定服务的 endpoint 就没有 baseUrl。打星是**项目级**的，没有模块级默认。
   */
  defaultBaseUrlDefinitionId: string | null;
  color: string;
  createdAt: number;
  updatedAt: number;
}

export interface Environment {
  id: string;
  projectId: string;
  name: string;
  /**
   * @deprecated 扁平值层，不再读写。唯一值层是 `Environment.moduleBaseUrls`。
   *
   * 键只有 `defId`，看不见模块，表达不了「同一服务在两个模块指向不同主机」。
   * 字段仍留在 schema 与线格式里：删列要重建表 = bump `SCHEMA_VERSION`
   * = `ensure_current_schema` 直接删库。迁移把历史值搬进 `moduleBaseUrls`，
   * 留在 `baseUrls` 的那份是死数据。
   */
  baseUrls: Record<string, string>;
  /**
   * 唯一的 URL 层：`serviceId → moduleId → URL`。
   *
   * 每个格子都是独立值，彼此**不继承也不回退**——缺省或空即"该组合没有值"，
   * 不会借用项目默认服务或别的模块的格子；两个模块能对同一服务名配不同主机
   * 就靠这一点。`resolveEndpointBaseUrl` 读 `moduleBaseUrls[def.id][moduleId]`。
   */
  moduleBaseUrls: Record<string, Record<string, string>>;
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
