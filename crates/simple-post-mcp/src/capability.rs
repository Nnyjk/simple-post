// 客户端能力校验：crates/simple-post-mcp/src/capability.rs
//
// `--capability`（及 `SIMPLE_POST_MCP_CAPABILITY`）在本地上再过滤一次
// `tools/list`，后端已按 `mcp.policy` 过滤过一层。
//
// 每个工具需要哪一档 `Capability`，由本 crate 的工具表
// `crate::mcp::REQUIRED_CAPABILITY` 回答；档位比较交给
// `simple_post_core::permissions::CapabilityPolicy::satisfies`，与后端
// `tools/call` 预检是同一份实现（`Read < Create < Update < Delete`，`Send` 只由
// `FullAccess` 覆盖）。故本地清单与后端裁决不会给出不一致的答案。
//
// 表里没有的工具名一律不放行：那种名字要么是未注册的，要么是注册表与能力表已经分叉，
// 两种情况下后端的两道门也拒绝它。
//
// 这层只收窄、不放宽：真正的裁决权在后端，本地开关是给「服务端已放开但仍不想让
// AI 看见破坏性工具」的用户用的。

use std::str::FromStr;

use simple_post_core::permissions::CapabilityPolicy;

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

impl CapabilityOverride {
    /// 本地档位对应的后端策略。两者一一对应，故过滤只需要一张档位比较表。
    fn policy(self) -> CapabilityPolicy {
        match self {
            Self::ReadOnly => CapabilityPolicy::ReadOnly,
            Self::SafeWrite => CapabilityPolicy::SafeWrite,
            Self::FullAccess => CapabilityPolicy::FullAccess,
        }
    }
}

/// 工具 `name` 在 `cap` 档位下是否对客户端可见。
pub fn filter_tools(name: &str, cap: CapabilityOverride) -> bool {
    match crate::mcp::required_capability(name) {
        Some(required) => cap.policy().satisfies(required),
        None => false,
    }
}

#[cfg(test)]
#[path = "capability_test.rs"]
mod tests;
