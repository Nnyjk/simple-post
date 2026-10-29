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
#[serde(rename_all = "camelCase")]
pub struct ProjectPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
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
    /// 每个格子独立：缺项表示「此模块在此环境没有地址」，解析到此为止，不会回落到别的
    /// 模块的值。格子里的 `proxy` 是实际访问地址，请求发出前用它替换 `url` 这个前缀。
    #[serde(default)]
    pub module_base_urls: HashMap<String, ModuleBaseUrl>,
    #[serde(default)]
    pub variables: HashMap<String, String>,
    /// 被停用的变量名。**照样存、照样显示，但不参与替换。**
    ///
    /// 与 `variables` 平行，而不是把值改成 `{value, enabled}`：后者会改掉
    /// `environments.variables_json` 列的既有格式，读路径
    /// `serde_json::from_str::<HashMap<String,String>>` 就会在旧行上失败。平行集合是纯增量：
    /// 旧行读回来是空集合，语义等价于「全部启用」。
    #[serde(default)]
    pub disabled_variables: Vec<String>,
    #[serde(default)]
    pub is_active: bool,
}

/// 一个（环境 × 模块）格子里的地址配置。
///
/// 两个字段回答两个问题：
///
/// - `url` 是**后端 api 地址**——文档里写的、模块对外声明的那个地址。
/// - `proxy` 是**实际访问地址**——接口经代理网关转代后真正要打的那个地址。
///
/// 两者不一致时，请求发出前用 `proxy` 替换 `url` 这个前缀，其余部分原样保留。故
/// `url = "http://svc.internal:8080/api/a"`、`proxy = "https://gw.corp.com/api/xxxx/a"`
/// 与端点路径 `/orders/list` 合成的实际请求是
/// `https://gw.corp.com/api/xxxx/a/orders/list`。
///
/// `proxy` 为 `None` = 没有代理，请求直接打 `url`。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleBaseUrl {
    /// 后端 api 地址。
    pub url: String,
    /// 实际访问地址。`None` = 直接用 `url`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 整张 `module_base_urls` map 替换。`db::environments::update` 以一次原子 `UPDATE`
    /// 写入该 JSON 列，故读者永远看不到半个 map。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_base_urls: Option<HashMap<String, ModuleBaseUrl>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables: Option<HashMap<String, String>>,
    /// 整份停用集合替换。缺席 = 不碰，值 = 替换成那份集合。
    ///
    /// 逐个变量的停用 / 启用走 `db::environments::set_var_disabled`：那是**一个**变量
    /// 的一次原子写，而整份替换会让「启用了谁」这件事看不见中间态。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_variables: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
}

impl EnvironmentPatch {
    /// 尚无写路径调用；存在是为了让下一个调用方不必自己手写字段表。见 [`serialises_to_empty_object`]。
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}
