//! `variable_fill` 测试共用的 fixture：两个模块、空环境、服务列表与最小端点。
//!
//! 文件名不以 `_test.rs` 结尾，所以 `mcp/mod_test.rs` 的两个源码扫描也会读到它。
//! 这里既不过门，也不丢弃任何 `Result`，因此不会成为它们的违规者。

use std::collections::HashMap;

use simple_post_core::domain::{
    AuthConfig, EndpointDetail, Environment, HttpMethod, Project, RequestBody,
};

use super::EndpointOverrides;
/// 被测端点所在的模块。之所以设两个模块，是为了让测试能证明模块选的是**格子** ——
/// 服务列表是项目级的，模块不再是「哪些服务存在」的过滤器。
pub(super) const MOD_A: &str = "mod_a";
/// 第二个模块，为同一个服务持有自己的地址。
pub(super) const MOD_B: &str = "mod_b";

pub(super) fn empty_env() -> Environment {
    Environment {
        id: "e1".into(),
        project_id: "p1".into(),
        name: "dev".into(),
        base_urls: HashMap::new(),
        module_base_urls: HashMap::new(),
        variables: HashMap::new(),
        is_active: true,
    }
}

/// Write one `(environment × service × module)` cell.
pub(super) fn set_cell(env: &mut Environment, def_id: &str, module: &str, url: &str) {
    env.module_base_urls
        .entry(def_id.to_string())
        .or_default()
        .insert(module.to_string(), url.to_string());
}

/// 项目的服务列表：项目级、扁平，`data` 持★。
pub(super) fn project_with_defs() -> Project {
    Project {
        id: "p1".into(),
        name: "demo".into(),
        description: None,
        base_url: None,
        base_url_definitions: vec![
            simple_post_core::domain::BaseUrlDefinition {
                id: "auth".into(),
                name: "auth".into(),
                description: None,
            },
            simple_post_core::domain::BaseUrlDefinition {
                id: "data".into(),
                name: "data".into(),
                description: None,
            },
        ],
        default_base_url_definition_id: Some("data".into()),
        color: "#000".into(),
        created_at: 0,
        updated_at: 0,
    }
}

/// 只带模块上下文的覆盖值，供那些关注替换、需要可解析 base URL 但没有草稿的测试用。
pub(super) fn in_module_a() -> EndpointOverrides {
    EndpointOverrides {
        module_id: Some(MOD_A.to_string()),
        ..Default::default()
    }
}

pub(super) fn minimal_endpoint() -> EndpointDetail {
    EndpointDetail {
        node_id: "ep1".into(),
        method: HttpMethod::Get,
        url: "/users/{{id}}".into(),
        environment_id: Some("e1".into()),
        base_url_definition_id: None,
        base_url_override: None,
        params: Vec::new(),
        headers: Vec::new(),
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
    }
}

/// 每个 (服务 × 模块) 各持一个不同 URL 的环境，好让「模块取错」无法伪装成「取对了」。
pub(super) fn env_with_two_modules() -> Environment {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://mod-a.example.com");
    set_cell(&mut env, "data", MOD_B, "https://mod-b.example.com");
    set_cell(&mut env, "auth", MOD_A, "https://mod-a-auth.example.com");
    env
}
