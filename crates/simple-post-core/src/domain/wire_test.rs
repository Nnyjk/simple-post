// 跨 IPC 的线格式往返：HTTP 方法、auth 与 oauth2 配置载荷。

use super::*;
use serde_json::json;

#[test]
fn http_method_round_trips_known_verbs() {
    for v in [
        HttpMethod::Get,
        HttpMethod::Post,
        HttpMethod::Put,
        HttpMethod::Patch,
        HttpMethod::Delete,
        HttpMethod::Head,
        HttpMethod::Options,
    ] {
        let s = serde_json::to_string(&v).unwrap();
        let back: HttpMethod = serde_json::from_str(&s).unwrap();
        assert_eq!(v, back);
    }
}

#[test]
fn http_method_serializes_uppercase_strings() {
    assert_eq!(serde_json::to_string(&HttpMethod::Get).unwrap(), "\"GET\"");
    assert_eq!(
        serde_json::to_string(&HttpMethod::Post).unwrap(),
        "\"POST\""
    );
}

#[test]
fn http_method_custom_verb_passes_through() {
    let raw = "\"PROPFIND\"";
    let m: HttpMethod = serde_json::from_str(raw).unwrap();
    assert_eq!(m.as_str(), "PROPFIND");
}

#[test]
fn apikey_location_field_round_trips_without_a_rename() {
    // TS 镜像读的是 `inLocation`。`in` 是 TypeScript 关键字，而 `in_` 不是解法——camelCase 会
    // 把它直接映射回 `in`。
    let v = json!({
        "kind": "apikey",
        "apikey": { "key": "X-API-Key", "value": "abc", "inLocation": "header" }
    });
    let auth: AuthConfig = serde_json::from_value(v).unwrap();
    let apikey = auth.apikey.clone().unwrap();
    assert_eq!(apikey.in_location, ApiKeyLocation::Header);
    // ……且发出的键正是镜像声明的那个词。
    let back = serde_json::to_value(&auth).unwrap();
    assert_eq!(back["kind"], json!("apikey"));
    assert_eq!(back["apikey"]["inLocation"], json!("header"));
}

/// `endpoint_details.auth_json` 里存着四年来的各代 `AuthConfig` 输出。新增鉴权方式时把
/// 新变体的字段设成必填，就是 AGENTS.md 记的「`type` → `kind` 改名后旧行读不出来」的
/// 同一类事故 —— 那次是靠 bump 版本删库才摆脱的。故每加一个 `AuthType` 变体都要过这一遍。
#[test]
fn every_auth_config_shape_written_before_today_still_deserialises() {
    for raw in [
        r#"{"kind":"none"}"#,
        r#"{"kind":"bearer","bearer":"eyJhbGciOi"}"#,
        r#"{"kind":"basic","basic":{"username":"u","password":"p"}}"#,
        r#"{"kind":"apikey","apikey":{"key":"X-K","value":"v","inLocation":"query"}}"#,
    ] {
        let auth: AuthConfig =
            serde_json::from_str(raw).unwrap_or_else(|e| panic!("{raw} 读不出来: {e}"));
        assert!(auth.oauth2.is_none(), "{raw} 不该凭空长出 oauth2 载荷");
    }
}

/// `'{}'` 仍是「读不出来」，这是 `schema.sql` 里写明的既定状态，**本改动不得顺手改掉
/// 它**：`kind` 必填是当年把 `type` 改名成 `kind` 时的选择，而行映射器把解析失败变成
/// 硬错误——放宽成「宽容读取」是一次独立的语义变更，不该被一个鉴权扩展顺带捎上。
///
/// 同时也是「新字段不得必填」的断言：`{}` 缺的是 `kind` 一个字段，若它开始报别的字段，
/// 说明有人给某个新变体加了必填字段。
#[test]
fn the_empty_object_still_fails_on_kind_alone() {
    let err = serde_json::from_str::<AuthConfig>("{}")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("missing field `kind`"),
        "'{{}}' 只该缺 kind 一个字段，got: {err}"
    );
}

/// 反过来也成立：只写 oauth2 的新行不该丢掉 `kind` —— 它是必填字段，正是当年因为
/// `kind` 必填而读不出 `'{}'` 的那个教训。
#[test]
fn an_oauth2_payload_alone_still_serialises_back() {
    let auth: AuthConfig = serde_json::from_value(json!({
        "kind": "oauth2",
        "oauth2": {
            "grantType": "client_credentials",
            "tokenUrl": "https://idp.example.com/token",
            "clientId": "cid"
        }
    }))
    .unwrap();
    let cfg = auth.oauth2.clone().unwrap();
    assert_eq!(cfg.grant_type, OAuth2GrantType::ClientCredentials);
    // 未给的字段取默认值，而不是反序列化失败。
    assert_eq!(cfg.client_auth, OAuth2ClientAuth::Basic);
    assert_eq!(cfg.token_location, OAuth2TokenLocation::Header);
    assert!(cfg.extra_params.is_empty());

    let back = serde_json::to_value(&auth).unwrap();
    assert_eq!(back["kind"], json!("oauth2"));
    assert_eq!(back["oauth2"]["grantType"], json!("client_credentials"));
    assert_eq!(
        back["oauth2"]["tokenUrl"],
        json!("https://idp.example.com/token")
    );
}

/// 线上取值就是用户在 JSON 里写的那个词。
#[test]
fn the_oauth2_wire_names_are_the_ones_a_user_writes() {
    assert_eq!(
        serde_json::to_string(&AuthType::Oauth2).unwrap(),
        "\"oauth2\""
    );
    assert_eq!(
        serde_json::to_string(&OAuth2GrantType::AuthorizationCode).unwrap(),
        "\"authorization_code\""
    );
    assert_eq!(
        serde_json::to_string(&OAuth2GrantType::ClientCredentials).unwrap(),
        "\"client_credentials\""
    );
    assert_eq!(
        serde_json::to_string(&OAuth2GrantType::Password).unwrap(),
        "\"password\""
    );
    assert_eq!(
        serde_json::to_string(&OAuth2GrantType::Implicit).unwrap(),
        "\"implicit\""
    );
    assert_eq!(
        serde_json::to_string(&OAuth2ClientAuth::Body).unwrap(),
        "\"body\""
    );
    assert_eq!(
        serde_json::to_string(&OAuth2TokenLocation::Query).unwrap(),
        "\"query\""
    );
    for kind in ["none", "bearer", "basic", "apikey", "oauth2"] {
        let parsed: AuthType = serde_json::from_str(&format!("\"{kind}\"")).unwrap();
        assert_eq!(parsed.as_str(), kind, "as_str 与线格式必须互为逆映射");
    }
}

/// 畸形流程值必须是错误，而不是退化成某个默认流程然后发出一条没人预期的请求。
/// serde 的报错会列出全部合法取值，AI 客户端据此自我纠正。
#[test]
fn an_unknown_oauth2_field_is_an_error_not_a_silent_default() {
    let err = serde_json::from_value::<OAuth2Auth>(json!({ "grantType": "teleport" }))
        .unwrap_err()
        .to_string();
    assert!(err.contains("teleport"), "报错要复述那个非法值，got: {err}");
    for known in [
        "authorization_code",
        "client_credentials",
        "password",
        "implicit",
    ] {
        assert!(
            err.contains(known),
            "报错要列出合法取值 `{known}`，got: {err}"
        );
    }
}

#[test]
fn endpoint_camel_case_field_names() {
    // 结构那一半与配置那一半现在是分开的两个类型，故这里检查两者都能从各自 TS 镜像预期的
    // camelCase 线形状反序列化。
    let v = json!({
        "id": "e1",
        "rootId": "p1",
        "parentId": "c1",
        "kind": "endpoint",
        "name": "foo",
        "sortOrder": 0,
        "createdAt": 1,
        "updatedAt": 2,
    });
    let n: ResourceNode = serde_json::from_value(v).unwrap();
    assert_eq!(n.parent_id.as_deref(), Some("c1"));
    assert_eq!(n.root_id, "p1");
    assert_eq!(n.kind, ResourceNodeType::Endpoint);
    assert_eq!(n.created_at, 1);

    let v = json!({
        "nodeId": "e1",
        "method": "GET",
        "url": "/x",
        "params": [],
        "headers": [],
        "body": { "mode": "none", "content": "" },
        "auth": { "kind": "none" },
        "tags": [],
    });
    let d: EndpointDetail = serde_json::from_value(v).unwrap();
    assert_eq!(d.node_id, "e1");
    assert_eq!(d.method, HttpMethod::Get);
    assert_eq!(d.url, "/x");
}

#[test]
fn http_method_has_one_parser_for_every_entry_point() {
    // 任何调用方给的小写都必须落到与 MCP 路径相同的存库动词上。派生的 `Deserialize` 会把
    // `"get"` 变成 `Custom("get")`，再原样小写往返出去并照此存库。
    for (json, want) in [
        ("\"get\"", HttpMethod::Get),
        ("\"PoSt\"", HttpMethod::Post),
        ("\"PROPFIND\"", HttpMethod::Custom("PROPFIND".into())),
        ("\"propfind\"", HttpMethod::Custom("PROPFIND".into())),
    ] {
        let m: HttpMethod = serde_json::from_str(json).unwrap();
        assert_eq!(m, want, "deserialising {json}");
        assert_eq!(m.as_str(), want.as_str());
        // 不管进来的是什么，出去的必是大写。
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            format!("\"{}\"", m.as_str())
        );
        // ……且 `parse_http_method` 与 serde 路径给出同一个答案。
        assert_eq!(parse_http_method(json.trim_matches('"')).unwrap(), want);
    }
}

#[test]
fn http_method_rejects_non_token_input() {
    for bad in ["", "   ", "GET POST", "GE\tT", "你好", "GET/1"] {
        assert!(
            parse_http_method(bad).is_err(),
            "{bad:?} should not parse as a method"
        );
        // 同样的拒绝也走 serde 路径——一个解析器，一个判决。
        let err = serde_json::from_str::<HttpMethod>(&format!("\"{bad}\""));
        assert!(err.is_err(), "serde should reject {bad:?}");
    }
}
