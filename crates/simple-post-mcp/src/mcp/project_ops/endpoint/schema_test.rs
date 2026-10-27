//! JSON Schema 必须自描述 —— agent 只读 `tools/list` 就得能写对请求。

use super::input::{parse_body_mode, AUTH_TYPE_VALUES, BODY_MODE_VALUES};
// =========================================================================
// 6. The JSON Schema is self-describing
// =========================================================================

/// An agent reading only `tools/list` cannot guess a nested shape. The
/// schema is the contract it has, so the item shape and the enum values
/// have to be in it.
#[test]
fn endpoint_create_schema_documents_the_item_shape_and_enums() {
    let descriptor = crate::mcp::tool_table()
        .iter()
        .find(|e| e.name == "endpoint_create")
        .map(|e| e.schema.clone())
        .expect("endpoint_create must be registered");
    let props = &descriptor["properties"];

    // `params` / `headers` share one `$defs` entry, so the item shape is
    // 完整写出再被引用。关键在于它**可解析** —— 跟着 `$ref` 走的 agent 必须落到
    // 一个真实携带字段名的定义上，而不是一个光秃秃的
    // `{"type":"object"}`。
    let params_ref = props["params"]["items"]["$ref"]
        .as_str()
        .expect("params items must $ref the keyValue definition");
    assert_eq!(params_ref, "#/$defs/keyValue");
    assert_eq!(
        props["headers"]["items"]["$ref"].as_str(),
        Some(params_ref),
        "headers must share the params item definition"
    );

    let defs = &descriptor["$defs"];
    let item_props = &defs["keyValue"]["properties"];
    assert_eq!(
        item_props["key"]["type"], "string",
        "the KeyValue item shape must be spelled out, not a bare object"
    );
    assert_eq!(item_props["value"]["type"], "string");
    assert_eq!(item_props["enabled"]["type"], "boolean");
    assert_eq!(
        defs["keyValue"]["required"],
        serde_json::json!(["key", "value"])
    );

    // 枚举必须与解析器实际接受的完全一致。
    // derived from `domain::BodyMode` / `domain::AuthType`.
    assert_eq!(
        props["body_mode"]["enum"],
        serde_json::json!(BODY_MODE_VALUES)
    );
    // 与解析器实际接受的那一份常量比对，而不是在这里再抄一遍：抄一份就多一个
    // 「加了变体却忘了改这里」的地方，而那种不一致对 agent 是静默的。
    assert_eq!(
        props["auth_type"]["enum"],
        serde_json::json!(AUTH_TYPE_VALUES)
    );
    // schema 声明的每个取值都必须能解析。
    for mode in props["body_mode"]["enum"].as_array().expect("enum") {
        let s = mode.as_str().expect("string");
        assert!(
            parse_body_mode(s).is_ok(),
            "schema advertises body_mode {s:?} but the parser rejects it"
        );
    }
}

/// An agent writing `body_content` has nothing but the enum and the prose.
/// A mode the prose never names is one it will fill with the wrong shape —
/// and a mode whose content is parsed rather than sent (multipart) then
/// yields zero parts with no error, so nothing downstream complains.
#[test]
fn every_advertised_body_mode_is_described_in_prose() {
    let descriptor = crate::mcp::tool_table()
        .iter()
        .find(|e| e.name == "endpoint_create")
        .map(|e| e.schema.clone())
        .expect("endpoint_create must be registered");
    let props = &descriptor["properties"];

    let prose = format!(
        "{} {}",
        props["body_mode"]["description"]
            .as_str()
            .unwrap_or_default(),
        props["body_content"]["description"]
            .as_str()
            .unwrap_or_default()
    );
    for mode in props["body_mode"]["enum"].as_array().expect("enum") {
        let s = mode.as_str().expect("string");
        assert!(
            prose.contains(s),
            "body_mode {s:?} is advertised but neither body_mode nor body_content describes it"
        );
    }
}

/// `endpoint_update` is the tool an agent uses to build interface
/// parameters, so its patch object must not be an untyped `{"type":
/// "object"}` either.
#[test]
fn endpoint_update_schema_documents_every_patchable_field() {
    let descriptor = crate::mcp::tool_table()
        .iter()
        .find(|e| e.name == "endpoint_update")
        .map(|e| e.schema.clone())
        .expect("endpoint_update must be registered");
    let patch_props = &descriptor["properties"]["patch"]["properties"];

    // Every field `tool_endpoint_update` accepts, and no field it does
    // —— 两份列表一旦漂移，schema 就是在撒谎。
    let mut documented: Vec<&str> = patch_props
        .as_object()
        .expect("patch properties")
        .keys()
        .map(String::as_str)
        .collect();
    documented.sort_unstable();
    assert_eq!(
        documented,
        vec![
            "auth",
            "base_url_definition_id",
            "base_url_override",
            "body",
            "description",
            "docs",
            "environment_id",
            "headers",
            "method",
            "name",
            "notes",
            "params",
            "path_params",
            "pre_request_script",
            "sort_order",
            "tags",
            "test_script",
            "url",
        ],
        "the documented patch fields must match the fields the handler reads"
    );

    assert_eq!(
        patch_props["body"]["properties"]["mode"]["enum"],
        serde_json::json!(BODY_MODE_VALUES)
    );
    assert_eq!(
        patch_props["auth"]["properties"]["kind"]["enum"],
        serde_json::json!(AUTH_TYPE_VALUES)
    );
    // oauth2 的载荷必须登记进 schema。`auth` 上有 `additionalProperties: false`，
    // 漏掉它不是「宽松一点」，而是 agent 根本传不进来——一个看着正常却永远收不到
    // 配置的端点。
    let oauth2 = &patch_props["auth"]["properties"]["oauth2"];
    assert_eq!(oauth2["type"], "object");
    assert_eq!(
        oauth2["properties"]["grantType"]["enum"],
        serde_json::json!([
            "authorization_code",
            "client_credentials",
            "password",
            "implicit"
        ])
    );
    for field in [
        "tokenUrl",
        "clientId",
        "clientSecret",
        "clientAuth",
        "scope",
        "authorizationUrl",
        "redirectUri",
        "codeVerifier",
        "authCode",
        "username",
        "password",
        "extraParams",
        "tokenLocation",
        "tokenParam",
    ] {
        assert!(
            oauth2["properties"].get(field).is_some(),
            "oauth2 载荷漏登记了 `{field}`"
        );
    }
    assert_eq!(oauth2["additionalProperties"], false);
    // 可空字段必须写明 `null` 表示清空 ——
    // three-state meaning is invisible on the wire otherwise.
    assert_eq!(patch_props["environment_id"]["type"][1], "null");
    assert_eq!(patch_props["base_url_override"]["type"][1], "null");
}
