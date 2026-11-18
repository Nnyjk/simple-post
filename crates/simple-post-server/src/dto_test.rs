//! `RequestSettingsPatch` 的两条契约：写错键名必须报错；读回来的一整份设置能原样 PATCH 回去。
//!
//! 它是五个补丁类型里唯一一个只有 `Deserialize` 的（Tauri 的 `request_settings_update` 只收
//! 局部更新），故身份字段不需要——[`RequestSettings`] 的四个字段名与补丁逐个一致。

use serde_json::json;

use super::{CreateEndpointDetailOpts, RequestSettingsPatch};
use simple_post_core::domain::{GraphQlRequest, RequestProtocol, RequestSettings, TlsSettings};

/// `POST /api/nodes` 带上 `protocol` / `graphql` 时必须落进 detail，而不是被静默忽略。
#[test]
fn create_opts_carry_protocol_and_graphql_into_the_detail() {
    let opts: CreateEndpointDetailOpts = serde_json::from_value(json!({
        "method": "GET",
        "url": "/graphql",
        "protocol": "graphql",
        "graphql": {
            "query": "{ ping }",
            "variables": "{\"id\": 1}",
            "operationName": "Ping"
        }
    }))
    .expect("protocol and graphql are accepted");

    let detail = opts.into_detail("e1");
    assert_eq!(detail.protocol, RequestProtocol::GraphQl);
    assert_eq!(
        detail.graphql,
        Some(GraphQlRequest {
            query: "{ ping }".into(),
            variables: "{\"id\": 1}".into(),
            operation_name: Some("Ping".into()),
        })
    );
}

#[test]
fn create_opts_reject_an_unknown_protocol_instead_of_defaulting_to_http() {
    let err = serde_json::from_value::<CreateEndpointDetailOpts>(json!({
        "url": "/x",
        "protocol": "websockets"
    }))
    .unwrap_err();
    assert!(err.to_string().contains("websockets"), "got {err}");
}

#[test]
fn create_opts_reject_unknown_graphql_keys_and_blank_queries() {
    let err = serde_json::from_value::<CreateEndpointDetailOpts>(json!({
        "url": "/graphql",
        "protocol": "graphql",
        "graphql": { "query": "{ ping }", "operation": "Ping" }
    }))
    .unwrap_err();
    assert!(err.to_string().contains("operation"), "got {err}");

    let err = serde_json::from_value::<CreateEndpointDetailOpts>(json!({
        "url": "/graphql",
        "protocol": "graphql",
        "graphql": { "query": "   " }
    }))
    .unwrap_err();
    assert!(err.to_string().contains("graphql.query"), "got {err}");
}

#[test]
fn a_misspelled_setting_is_rejected() {
    // `timeoutMS` 是很容易写错的那个键。静默丢掉未知键的话，调用方拿到 200、设置一字未改，
    // 而它以为改成功了。
    let err =
        serde_json::from_value::<RequestSettingsPatch>(json!({ "timeoutMS": 1_000 })).unwrap_err();
    assert!(err.to_string().contains("timeoutMS"), "got {err}");
}

#[test]
fn a_settings_read_back_can_be_written_back_as_a_patch() {
    let settings = RequestSettings {
        timeout_ms: 12_000,
        follow_redirects: false,
        max_response_size_kb: 900,
        tls: TlsSettings {
            skip_verify: true,
            ..TlsSettings::default()
        },
    };
    let patch: RequestSettingsPatch =
        serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
    assert_eq!(patch.timeout_ms, Some(12_000));
    assert_eq!(patch.follow_redirects, Some(false));
    assert_eq!(patch.max_response_size_kb, Some(900));
    assert_eq!(patch.tls.map(|t| t.skip_verify), Some(true));
}
