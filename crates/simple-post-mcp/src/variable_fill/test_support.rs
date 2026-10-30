//! `variable_fill` 测试共用的 fixture：两个模块、空环境与最小端点。

use std::collections::HashMap;

use simple_post_core::domain::{
    AuthConfig, EndpointDetail, Environment, HttpMethod, ModuleBaseUrl, RequestBody,
};

use super::EndpointOverrides;
/// 被测端点所在的模块。之所以设两个模块，是为了让测试能证明模块选的是**格子** ——
/// 两个模块的地址互不相干，任何一个都不借用另一个的。
pub(super) const MOD_A: &str = "mod_a";
/// 第二个模块，持有自己的地址。
pub(super) const MOD_B: &str = "mod_b";

pub(super) fn empty_env() -> Environment {
    Environment {
        id: "e1".into(),
        project_id: "p1".into(),
        name: "dev".into(),
        module_base_urls: HashMap::new(),
        variables: HashMap::new(),
        disabled_variables: Vec::new(),
        created_at: 0,
    }
}

/// 写一个（环境 × 模块）格子。
pub(super) fn set_cell(env: &mut Environment, module: &str, url: &str) {
    env.module_base_urls.insert(
        module.to_string(),
        ModuleBaseUrl {
            url: url.to_string(),
            proxy: None,
        },
    );
}

/// 写一个带代理的（环境 × 模块）格子。
pub(super) fn set_cell_with_proxy(env: &mut Environment, module: &str, url: &str, proxy: &str) {
    env.module_base_urls.insert(
        module.to_string(),
        ModuleBaseUrl {
            url: url.to_string(),
            proxy: Some(proxy.to_string()),
        },
    );
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
        base_url_override: None,
        params: Vec::new(),
        path_params: Vec::new(),
        headers: Vec::new(),
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
        tls: None,
    }
}

/// 两个模块各持一个不同 URL 的环境，好让「模块取错」无法伪装成「取对了」。
pub(super) fn env_with_two_modules() -> Environment {
    let mut env = empty_env();
    set_cell(&mut env, MOD_A, "https://mod-a.example.com");
    set_cell(&mut env, MOD_B, "https://mod-b.example.com");
    env
}
