//! `securitySchemes` → `AuthConfig`：能映射的落 auth，映射不了的出声。

use super::support::{ir_of, warns};

#[test]
fn mappable_security_schemes_become_auth_and_the_rest_are_reported() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "components": {
            "securitySchemes": {
              "bearerAuth": { "type": "http", "scheme": "bearer" },
              "cookieKey": { "type": "apiKey", "name": "sid", "in": "cookie" },
              "oidc": { "type": "openIdConnect", "openIdConnectUrl": "https://example.com/.well-known" }
            }
          },
          "paths": {
            "/a": { "get": { "security": [ { "cookieKey": [] } ], "responses": { "200": {} } } },
            "/b": { "get": { "security": [ { "bearerAuth": [] } ], "responses": { "200": {} } } },
            "/c": { "get": { "security": [ { "oidc": [] } ], "responses": { "200": {} } } }
          }
        }"#,
    );
    let by_path = |p: &str| -> &crate::ir::IrEndpoint {
        ex.modules
            .iter()
            .flat_map(|m| m.endpoints.iter())
            .find(|e| e.path == p)
            .unwrap_or_else(|| panic!("没有 {p}"))
    };
    assert_eq!(
        by_path("/a").auth.kind,
        simple_post_core::domain::AuthType::None,
        "cookie apiKey 映射不了，端点上就没有 auth"
    );
    assert_eq!(
        by_path("/b").auth.kind,
        simple_post_core::domain::AuthType::Bearer
    );
    assert_eq!(
        by_path("/c").auth.kind,
        simple_post_core::domain::AuthType::None
    );
    assert_eq!(warns(&ex).len(), 2, "cookie 与 openIdConnect 各一条");
}
