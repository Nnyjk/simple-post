//! `parse` 的顶层守卫与 `variable[]` → 环境。

use serde_json::json;
use simple_post_core::error::AppError;

use super::parse;
use super::support::{collection_fixture, parse_ok};

#[test]
fn a_non_object_document_is_rejected_rather_than_panicking() {
    assert!(matches!(
        parse(&json!([1, 2, 3])),
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        parse(&json!("nope")),
        Err(AppError::Validation(_))
    ));
}

#[test]
fn collection_variables_become_an_environment_not_a_service() {
    let parsed = parse_ok(&collection_fixture());
    assert_eq!(parsed.environments.len(), 1);
    let env = &parsed.environments[0];
    assert!(!env.id.is_empty());
    // 落库方填 project_id，导入不猜。
    assert_eq!(env.project_id, "");
    assert_eq!(env.name, "订单服务");
    assert_eq!(
        env.variables.get("base").map(String::as_str),
        Some("https://api.example.com")
    );
    // 变量不是服务：导成服务会凭空造出一台不存在的机器。
    assert!(env.base_urls.is_empty());
    assert!(env.module_base_urls.is_empty());
    assert!(!env.is_active, "导入不抢激活位");

    // 没有 variable[] 就一个环境都不产出。
    let bare = parse_ok(&json!({ "info": { "name": "n" }, "item": [] }));
    assert!(bare.environments.is_empty());
}
