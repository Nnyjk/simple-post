//! 项目及其环境：两者都是项目级聚合的根，URL 值层挂在环境上。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::util::serialises_to_empty_object;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "super::default_project_color")]
    pub color: String,
    /// 服务端时间戳；调用方（如 REST 的 `POST /api/projects` handler）不给时取 `now_ms()`。
    /// Tauri command 一律显式设置，故对它们而言该默认值是惰性的。
    #[serde(default = "super::now_ms")]
    pub created_at: i64,
    #[serde(default = "super::now_ms")]
    pub updated_at: i64,
}

impl Project {
    /// 空壳。查不到项目时用它占位：解析器要一个 `&Project`，而「没有项目」是一个
    /// 真实答案——解析出空 base URL，不猜。
    pub fn empty() -> Project {
        Project {
            id: String::new(),
            name: String::new(),
            description: None,
            color: super::default_project_color(),
            created_at: 0,
            updated_at: 0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// 项目自己的身份字段：`id` / `createdAt` / `updatedAt`。
    ///
    /// 「取回再改再写回」是公开契约（完整理由见 `domain::resource::ResourceNodePatch` 上
    /// 的说明）：客户端 GET 一个项目，改一处，再把整个 body PATCH 回去。补丁类型带
    /// `deny_unknown_fields`，故它必须认得项目自己的每一个字段名，否则这条往返从一个 400
    /// 开始。这几个字段只被**接受**，不被写入：身份由路径里的 id 决定，创建与更新时间由
    /// 写路径自己盖。用 `skip_serializing`（而非 `skip_serializing_if`）是因为它们不指向
    /// 任何可写列，不该让 [`ProjectPatch::is_empty`] 读起来像「有东西要写」。
    #[serde(default, skip_serializing)]
    pub id: Option<String>,
    #[serde(default, skip_serializing)]
    pub created_at: Option<i64>,
    #[serde(default, skip_serializing)]
    pub updated_at: Option<i64>,
}

impl ProjectPatch {
    /// 尚无写路径调用；存在是为了让下一个调用方不必自己手写字段表。见 [`serialises_to_empty_object`]。
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}

// environment

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub id: String,
    pub project_id: String,
    pub name: String,
    /// `module_id -> ModuleBaseUrl`。**URL 只存在于这一层。**
    ///
    /// 每个格子独立：缺项表示「此模块在此环境没有地址」，解析到此为止。格子里的 `rewrite`
    /// 是路径前缀改写规则，请求发出前用它改写端点自己的 URL 路径。
    #[serde(default)]
    pub module_base_urls: HashMap<String, ModuleBaseUrl>,
    #[serde(default)]
    pub variables: HashMap<String, String>,
    /// 被停用的变量名。**照样存、照样显示，但不参与替换。**
    ///
    /// 与 `variables` 平行，而不是把值改成 `{value, enabled}`：后者会改掉
    /// `environments.variables_json` 列的既有格式，读路径就会在旧行上失败。平行集合是纯增量。
    #[serde(default)]
    pub disabled_variables: Vec<String>,
    /// 创建时刻（unix 毫秒）。
    ///
    /// 「默认环境」按它认定：**一个项目里最早创建的那个环境**是它的默认环境。默认环境是项目的
    /// 属性，而「当前环境」是每个客户端自己的实时状态——后者不落库，故库里没有「当前」这一列。
    #[serde(default)]
    pub created_at: i64,
}

/// 路径前缀改写规则：把端点**自己 URL** 的路径前缀从 `from` 换成 `to`。
///
/// - `from`（起点）是端点自己 URL 的路径前缀，如 `/api/air`。它**按原文匹配**：不做
///   `{{var}}` 替换；模块地址自带的路径段也不参与匹配（模块地址与端点地址互不干扰）。
/// - `to`（目标）是替换后的路径前缀，如 `/api/project-a/air`。空串表示把这段前缀整个去掉。
///
/// 命中要求落在路径边界上：URL 以 `from` 开头，**且**其后紧跟的那一位是 `/`、`?` 或
/// `#`（或 URL 恰好等于 `from`）。没有这一条，`/api/air` 会命中 `/api/airplane`。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RewriteRule {
    /// 被替换的路径前缀，原文照抄。
    pub from: String,
    /// 替换后的路径前缀。空串表示去掉这段前缀。
    pub to: String,
}

/// 一个（环境 × 模块）格子里的地址配置。
///
/// - `url` 是**模块地址**——文档里写的、模块对外声明的那个地址。
/// - `rewrites` 是**有序的路径前缀改写规则**——只改写端点自己的 URL 路径，模块地址里的主机与
///   路径段一概不动。按列表顺序依次匹配，命中第一条后不再向后匹配。
///
/// 两者回答两个不同的问题：前者决定打哪个主机，后者决定路径怎么改。主机不变、只换路径前缀
/// 这一类需求由 `rewrites` 表达。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleBaseUrl {
    /// 模块地址。
    pub url: String,
    /// 路径前缀改写规则列表，按顺序匹配，首条命中即停止。空表示没有改写。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rewrites: Vec<RewriteRule>,
}

/// [`ModuleBaseUrl`] 的读取形状：同时认得历史单个 `rewrite` 与现在的 `rewrites` 列表。
///
/// 旧库的 `environments.module_base_urls_json` 里存的是 `{ "url": …, "rewrite": {…} }`；
/// 直接换字段会让既有 store.db 读不出来。故读的时候把 `rewrite` 提升成单元素列表，使
/// `SCHEMA_VERSION` 不必上涨（上涨会删库）。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawModuleBaseUrl {
    url: String,
    #[serde(default)]
    rewrites: Option<Vec<RewriteRule>>,
    #[serde(default)]
    rewrite: Option<RewriteRule>,
}

impl<'de> Deserialize<'de> for ModuleBaseUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;
        let raw = RawModuleBaseUrl::deserialize(deserializer)?;
        let rewrites = match (raw.rewrites, raw.rewrite) {
            (Some(_), Some(_)) => {
                return Err(D::Error::custom(
                    "cannot specify both 'rewrite' and 'rewrites'",
                ))
            }
            (Some(list), None) => list,
            (None, Some(rule)) => vec![rule],
            (None, None) => Vec::new(),
        };
        Ok(ModuleBaseUrl {
            url: raw.url,
            rewrites,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvironmentPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 整张 `module_base_urls` map 替换。`db::environments::update` 以一次原子 `UPDATE`
    /// 写入该 JSON 列，故读者永远看不到半个 map。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_base_urls: Option<HashMap<String, ModuleBaseUrl>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables: Option<HashMap<String, String>>,
    /// 整份停用集合替换。缺席 = 不碰，值 = 替换成那份集合。逐个变量的停用 / 启用走
    /// `db::environments::set_var_disabled`：那是**一个**变量的一次原子写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_variables: Option<Vec<String>>,
    /// 环境自己的身份字段：`id` / `projectId` / `createdAt`。理由同
    /// [`ProjectPatch::id`]：认得，但不写。
    #[serde(default, skip_serializing)]
    pub id: Option<String>,
    #[serde(default, skip_serializing)]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing)]
    pub created_at: Option<i64>,
}

impl EnvironmentPatch {
    /// 尚无写路径调用；存在是为了让下一个调用方不必自己手写字段表。见 [`serialises_to_empty_object`]。
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}
