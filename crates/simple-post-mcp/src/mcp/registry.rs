//! 工具注册表与全局能力门。
//!
//! 注册表与 `REQUIRED_CAPABILITY` 两张表必须同步：注册了却没有 capability 行的
//! 工具在两道门里都被当作「未分类」，而未分类一律拒绝。

use simple_post_core::permissions::{resolve_capability, Capability, CapabilityPolicy};

use super::protocol::{ToolDescriptor, ToolEntry};
use super::state::AppState;
use super::{dynamic_ops, env_ops, http_ops, project_ops, response_parse, variable_fill};

/// 完整工具注册表。这里的顺序就是 `tools/list` 的顺序，按类别分组，方便人在
/// `/mcp/tools` 里看。
pub(crate) fn tool_table() -> &'static [ToolEntry] {
    static TABLE: std::sync::OnceLock<Vec<ToolEntry>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut v = Vec::with_capacity(64);

        // env_ops：7 个
        v.extend(env_ops::table());

        // project_ops：project + module + collection + endpoint + spec
        v.extend(project_ops::table());

        // http_ops：2 个
        v.extend(http_ops::table());

        // variable_fill（MCP 适配层）：3 个
        v.extend(variable_fill::table());

        // response_parse（MCP 适配层）：3 个
        v.extend(response_parse::table());

        // dynamic_ops：动态变量 7 个
        v.extend(dynamic_ops::table());

        v
    })
}

/// 全部工具名的快照（给 `/mcp/healthz` 与自检）。
///
/// **注意**这是未过滤的全量注册表，不是用户在当前策略下看得见的那份；要
/// 「按策略之后」的那份请用 [`tool_descriptors`]。healthz 与 metrics 都
/// 不需要用户的视角。
pub fn registered_tool_names() -> Vec<String> {
    tool_table().iter().map(|e| e.name.to_string()).collect()
}

/// 工具名 → 它需要的全局 [`Capability`]。
///
/// **穷尽**：`tool_table()` 里每一条都必须在此有行。两道能力门在缺行时都
/// *fail closed*，所以少一行的工具是被拒绝，而不是被默认放行；
/// `every_registered_tool_has_a_capability` 是让两张表同步的回归网。
///
/// 用户全局 `read_only` / `safe_write` / `full_access` 的执行点在这里，
/// 因此本表不能有默认值。按项目的
/// [`simple_post_core::permissions::required_permission`] 是另一道更粗的门
/// （未知工具默认 `Permission::Full`），两者不要互相顶替。
pub const REQUIRED_CAPABILITY: &[(&str, Capability)] = &[
    // env
    ("env_list", Capability::Read),
    ("env_get_default", Capability::Read),
    ("env_create", Capability::Create),
    ("env_update", Capability::Update),
    ("env_delete", Capability::Delete),
    ("env_set_var", Capability::Update),
    ("env_set_baseurl", Capability::Update),
    // project
    ("project_list", Capability::Read),
    ("project_get", Capability::Read),
    ("project_create", Capability::Create),
    ("project_update", Capability::Update),
    ("project_delete", Capability::Delete),
    // module
    ("module_list", Capability::Read),
    ("module_create", Capability::Create),
    ("module_update", Capability::Update),
    ("module_delete", Capability::Delete),
    ("module_move", Capability::Update),
    // collection
    ("collection_list", Capability::Read),
    ("collection_create", Capability::Create),
    ("collection_update", Capability::Update),
    ("collection_delete", Capability::Delete),
    ("collection_move", Capability::Update),
    // endpoint
    ("endpoint_list", Capability::Read),
    ("endpoint_get", Capability::Read),
    ("endpoint_search", Capability::Read),
    ("endpoint_create", Capability::Create),
    ("endpoint_update", Capability::Update),
    ("endpoint_delete", Capability::Delete),
    ("endpoint_duplicate", Capability::Create),
    ("endpoint_move", Capability::Update),
    // 整份接口规格。取 `Update` 而不是 `Create`：它不但建节点，还会改环境变量、
    // 改 base URL 那一格，`if_exists=update` 时还会覆盖已有端点的字段。
    ("spec_upsert", Capability::Update),
    // HTTP 执行 —— 唯三会发外网请求的工具，信任级别最高。
    ("api_run_request", Capability::Send),
    ("api_call_endpoint", Capability::Send),
    ("api_run_collection", Capability::Send),
    // 读侧的变量 / 端点辅助
    ("api_resolve_template", Capability::Read),
    ("api_resolve_endpoint", Capability::Read),
    ("api_validate_endpoint", Capability::Read),
    ("api_parse_response", Capability::Read),
    ("api_diff_responses", Capability::Read),
    ("api_extract_jsonpath", Capability::Read),
    // 动态变量。`dynamic_var_resolve` 会真的发出站请求，故与 `api_call_endpoint` 同级。
    ("dynamic_var_list", Capability::Read),
    ("dynamic_var_get", Capability::Read),
    ("dynamic_var_create", Capability::Create),
    ("dynamic_var_update", Capability::Update),
    ("dynamic_var_delete", Capability::Delete),
    ("dynamic_var_resolve", Capability::Send),
    ("dynamic_var_set_override", Capability::Update),
];

/// 在小表 `REQUIRED_CAPABILITY` 上线性查。未知名返回 `None`，调用方必须自己
/// 走「拒绝」分支。表只有「每工具一行」这么大，哈希表属于过早优化。
pub fn required_capability(name: &str) -> Option<Capability> {
    REQUIRED_CAPABILITY
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
}

/// 全部工具描述符的快照（给 `tools/list` 与 `/mcp/tools`）。按当前
/// [`CapabilityPolicy`] 滤掉策略不覆盖的工具 —— 这是 LLM 可见清单的唯一真相，
/// 配错的策略绝不能把写工具泄给 AI。
///
/// 未设置策略时取 [`CapabilityPolicy::ReadOnly`]（safe-by-default）：没进过
/// 设置面板的用户只看到只读清单。
pub async fn tool_descriptors(state: &AppState) -> Vec<ToolDescriptor> {
    let policy = resolve_capability(&state.pool, None)
        .await
        .unwrap_or(CapabilityPolicy::default());
    tool_table()
        .iter()
        .filter(|e| {
            // fail **closed**：注册了却没有 capability 行的工具是注册表 bug，
            // 藏起来比把用户没批准过的写工具暴露给 AI 客户端安全。
            required_capability(e.name)
                .map(|req| policy.satisfies(req))
                .unwrap_or(false)
        })
        .map(|e| ToolDescriptor {
            name: e.name.to_string(),
            description: e.description.to_string(),
            input_schema: e.schema.clone(),
        })
        .collect()
}
