//! `servers[0].url` 的 host / basePath 拆分，以及多服务器降级。

use super::support::{dropped, first_endpoint, ir_of};

#[test]
fn the_server_url_splits_into_a_host_and_a_path_prefix() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "servers": [ { "url": "https://api.example.com/v2" } ],
          "paths": { "/users": { "get": { "responses": { "200": {} } } } }
        }"#,
    );
    let ep = first_endpoint(&ex);
    assert_eq!(ep.path, "/v2/users", "url 字段只存 path，host 不进去");
    assert_eq!(ex.modules[0].servers.len(), 1);
    assert_eq!(ex.modules[0].servers[0].url, "https://api.example.com");
    assert_eq!(ex.modules[0].servers[0].service, "api.example.com");
}

#[test]
fn a_second_server_entry_is_reported_because_the_model_has_one_url_per_module() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "servers": [
            { "url": "https://a.example.com" },
            { "url": "https://b.example.com" }
          ],
          "paths": { "/users": { "get": { "responses": { "200": {} } } } }
        }"#,
    );
    assert_eq!(ex.modules[0].servers[0].url, "https://a.example.com");
    assert_eq!(dropped(&ex), vec!["https://b.example.com"]);
}
