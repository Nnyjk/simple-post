// crates/simple-post-mcp/src/capability.rs
//
// `--capability`（及 `SIMPLE_POST_MCP_CAPABILITY`）在本地上再过滤一次
// `tools/list`，后端已按 `mcp.policy` 过滤过一层。
//
// 后端的 `CapabilityPolicy` 在 `simple-post-core::permissions`，这里拿不到，
// 于是镜像一份本地 enum：
//
//   read_only   → 只留 `Capability::Read`。响应体里没有该元数据，只能按名字
//                 里的 `_delete` / `_create` / `_update` / `_set_` /
//                 `_move` / `_duplicate` 反推。
//   safe_write  → read-only 之外放行安全改写（env_set_var、env_set_baseurl、
//                 env_set_active、base_url_def_set_default、request_settings_update）。
//   full_access → 不过滤，仍以后端策略为准。
//
// 这层只收窄、不放宽：真正的裁决权在后端，本地开关是给「服务端已放开但仍不想让
// AI 看见破坏性工具」的用户用的。

use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityOverride {
    ReadOnly,
    SafeWrite,
    FullAccess,
}

impl FromStr for CapabilityOverride {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "read_only" | "read-only" | "readonly" => Ok(Self::ReadOnly),
            "safe_write" | "safe-write" | "safewrite" => Ok(Self::SafeWrite),
            "full_access" | "full-access" | "fullaccess" => Ok(Self::FullAccess),
            other => Err(format!(
                "unknown capability `{other}` (expected read_only|safe_write|full_access)"
            )),
        }
    }
}

impl std::fmt::Display for CapabilityOverride {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ReadOnly => "read_only",
            Self::SafeWrite => "safe_write",
            Self::FullAccess => "full_access",
        })
    }
}

/// 破坏性工具的名字后缀。刻意不把后端的 `REQUIRED_CAPABILITY` 拉到客户端来，
/// 新增工具时在同一个 commit 里补一行即可。
const DESTRUCTIVE_SUFFIXES: &[&str] = &[
    "_create",
    "_update",
    "_delete",
    "_set_",
    "_move",
    "_duplicate",
];

/// `FullAccess` 在本地一律放行。
pub fn filter_tools(name: &str, cap: CapabilityOverride) -> bool {
    match cap {
        CapabilityOverride::FullAccess => true,
        CapabilityOverride::SafeWrite => !is_destructive(name),
        CapabilityOverride::ReadOnly => {
            // 会改状态或发外网的一律隐藏，只留安全的读。
            !is_destructive(name)
                && !name.contains("api_run_request")
                && !name.contains("api_call_endpoint")
                && !name.contains("api_resolve_endpoint")
                && !name.contains("api_validate_endpoint")
                // `dynamic_var_resolve` 会真的发出站请求（去数据源取数），故与
                // `api_call_endpoint` 同等对待。
                && !name.contains("dynamic_var_resolve")
        }
    }
}

fn is_destructive(name: &str) -> bool {
    DESTRUCTIVE_SUFFIXES.iter().any(|s| name.ends_with(s))
        || name == "api_diff_responses"
        || name == "api_extract_jsonpath"
        // `dynamic_var_set_override` 以 `_override` 结尾，匹配不到上面那张后缀表，
        // 但它写的是状态，故显式列出来。
        || name == "dynamic_var_set_override"
}

#[cfg(test)]
#[path = "capability_test.rs"]
mod tests;
