//! 项目及其环境：两者都是项目级聚合的根，URL 值层挂在环境上。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::resource::BaseUrlDefinition;
use super::util::serialises_to_empty_object;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default)]
    pub base_url_definitions: Vec<BaseUrlDefinition>,
    /// 项目的 ★ 服务。未指名服务的 endpoint 按它解析，故此处为 `None` 的项目发出这类请求时
    /// 没有 base URL，而不是借用别的项目或别的模块的。新建项目就是 NULL：不播种，因为用户
    /// 没点名的服务，是用户随后还得去删的服务。
    ///
    /// 刻意**不**加 `skip_serializing_if`：「本项目没有默认 baseUrl」是项目做出的决定，不是
    /// 数据缺席；TypeScript 镜像把它声明为必填但可空（`defaultBaseUrlDefinitionId:
    /// string | null`），发 `null` 两边才讲同一个故事，省掉键则让一个已声明的契约字段以
    /// `undefined` 到达。本结构体其余可选字段在镜像里确实可选，故保留 skip。
    #[serde(default)]
    pub default_base_url_definition_id: Option<String>,
    #[serde(default = "super::default_project_color")]
    pub color: String,
    /// 服务端时间戳；调用方（如 REST 的 `POST /api/projects` handler）不给时取 `now_ms()`。
    /// Tauri command 一律显式设置，故对它们而言该默认值是惰性的。
    #[serde(default = "super::now_ms")]
    pub created_at: i64,
    #[serde(default = "super::now_ms")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url_definitions: Option<Vec<BaseUrlDefinition>>,
    /// 项目的 ★。`Some(None)` 清空它，这是真实状态而非省略：渲染器在列出服务的同一个菜单里
    /// 提供「无默认服务」，而未指名服务的 endpoint 于是解析不到任何 base URL。
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_base_url_definition_id: Option<Option<String>>,
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
    /// **已废弃——仅为线兼容而读，无人写入。**
    ///
    /// 它曾是 `def_id -> url`，即 URL 还不关心 endpoint 挂在哪个模块时的值层。现役层是
    /// [`Self::module_base_urls`]；`db::migrate` 已把每个值搬过去，且是本 crate 里仅剩的
    /// 该列读者。
    ///
    /// 留在结构体上，好让旧客户端的载荷仍能反序列化（镜像声明为可选），但
    /// `db::environments::update` 忽略它。静默丢弃是代价，此处即是记录。
    #[serde(default)]
    pub base_urls: HashMap<String, String>,
    /// `def_id -> module_id -> url`。**URL 只存在于这一层。**
    ///
    /// 每个单元格独立：缺项表示「此模块对此服务无地址」，解析到此为止，不会回落到环境级
    /// map 或别的模块的值。故该 map 之所以按模块做键：两个模块完全可以把同一服务指向不同
    /// host，而空单元格是一个真实答案，不是一个洞。
    #[serde(default)]
    pub module_base_urls: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    pub variables: HashMap<String, String>,
    /// 被停用的变量名。**照样存、照样显示，但不参与替换。**
    ///
    /// 与 `variables` 平行，而不是把值改成 `{value, enabled}`：后者会改掉
    /// `environments.variables_json` 列的既有格式，读路径
    /// `serde_json::from_str::<HashMap<String,String>>` 就会在旧行上失败——那是要数据
    /// 迁移的地带。平行集合是纯增量：旧行读回来是空集合，语义等价于「全部启用」。
    #[serde(default)]
    pub disabled_variables: Vec<String>,
    #[serde(default)]
    pub is_active: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 随 [`Environment::base_urls`] 一起废弃：只收下以让旧载荷仍能反序列化，从不写入——
    /// 该列已死。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_urls: Option<HashMap<String, String>>,
    /// 整张 `module_base_urls` map 替换。`db::environments::update` 以一次原子 `UPDATE`
    /// 写入该 JSON 列，故读者永远看不到半个 map。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_base_urls: Option<HashMap<String, HashMap<String, String>>>,
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
