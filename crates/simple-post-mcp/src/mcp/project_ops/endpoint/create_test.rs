//! 请求配置面必须完整且写得进去，外加端点 id 的唯一性。
//!
//! 每个字段都断言在**存储行**里，而不是只看回显的响应。

use super::test_support::{bare_project, detail_of, fixture, node, state};
use super::write::{tool_endpoint_create, tool_endpoint_update};
use simple_post_core::db::{nodes, permissions, projects};
use simple_post_core::domain::{BodyMode, HttpMethod, ResourceNodeType};
use simple_post_core::error::AppError;
use simple_post_core::permissions::Permission;
// =========================================================================
// 2. 「接口参数」的写入面必须完整可写
// =========================================================================

/// 这条用例覆盖整次改动要解决的事：一个 agent 用 `endpoint_create` +
/// `endpoint_update` 就能把端点完整的请求配置搭起来，并且每一项都在**存储行**里核对，
/// 不是核对回显。
#[tokio::test]
async fn an_agent_can_build_every_piece_of_endpoint_request_config() {
    let (st, id) = fixture();

    // --- 经 endpoint_update 写入完整配置面 ---------
    tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": {
                "method": "POST",
                "url": "/api/users/{id}",
                "params": [
                    { "key": "page", "value": "1" },
                    { "key": "size", "value": "20", "enabled": false }
                ],
                "headers": [
                    { "key": "Content-Type", "value": "application/json" },
                    { "key": "X-Trace", "value": "abc" }
                ],
                "body": {
                    "mode": "json",
                    "content": "{\"name\":\"ada\"}",
                    "schema": "{\"name\":\"string\"}"
                },
                "auth": {
                    "kind": "bearer",
                    "bearer": "tok-123"
                },
                "tags": ["smoke", "user"],
                "base_url_override": "https://staging.example.com",
                "docs": "Creates a user.",
                "notes": "checked by hand",
                "name": "create user"
            }
        }),
    )
    .await
    .expect("a well-formed full patch must be accepted");

    let d = detail_of(&st, &id);

    assert_eq!(d.method, HttpMethod::Post);
    assert_eq!(d.url, "/api/users/{id}");

    // Query 参数：这两行没写 `id` / `enabled`，正是 agent 会写出的形状。
    // 转换器给前者补铸一个行 id、把后者默认成 `true`；存成禁用的请求头，
    // 就是一个永远发不出去的请求头。
    assert_eq!(d.params.len(), 2);
    let page = d.params.iter().find(|kv| kv.key == "page").expect("page");
    assert_eq!(page.value, "1");
    assert!(page.enabled, "an omitted `enabled` must default to true");
    assert!(!page.id.is_empty(), "a row id must be minted");
    let size = d.params.iter().find(|kv| kv.key == "size").expect("size");
    assert!(
        !size.enabled,
        "an explicit `enabled: false` must still be honoured"
    );

    assert_eq!(d.headers.len(), 2);
    assert!(d.headers.iter().all(|kv| kv.enabled));
    assert!(d
        .headers
        .iter()
        .any(|kv| kv.key == "Content-Type" && kv.value == "application/json"));

    // Body：mode、content、schema 三项都写进去了，不是只有前两项。
    assert_eq!(d.body.mode, BodyMode::Json);
    assert_eq!(d.body.content, "{\"name\":\"ada\"}");
    assert_eq!(d.body.schema.as_deref(), Some("{\"name\":\"string\"}"));

    // Auth：类型与配置。
    assert_eq!(
        d.auth.kind,
        simple_post_core::domain::AuthType::Bearer,
        "auth type must be settable"
    );
    assert_eq!(d.auth.bearer.as_deref(), Some("tok-123"));

    assert_eq!(d.tags, vec!["smoke".to_string(), "user".to_string()]);

    // 端点自己的 base URL。
    assert_eq!(
        d.base_url_override.as_deref(),
        Some("https://staging.example.com")
    );

    // docs 与 notes 走 patch 写入：patch 里不带这两个字段，它们的值就一直是空。

    assert_eq!(d.docs.as_deref(), Some("Creates a user."));
    assert_eq!(d.notes.as_deref(), Some("checked by hand"));

    let n = simple_post_core::db::with_conn(&st.pool, |c| nodes::get(c, &id)).expect("node");
    assert_eq!(n.name, "create user");
}

/// `double_option` 字段上 `Some(None)` 必须仍表示「清空」：patch 若只能设值，
/// `base_url_override` / `notes` 就变成单向可写。
#[tokio::test]
async fn an_explicit_null_clears_a_nullable_patch_field() {
    let (st, id) = fixture();
    tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": { "base_url_override": "https://one.example", "notes": "first" }
        }),
    )
    .await
    .expect("setting the fields");
    assert_eq!(
        detail_of(&st, &id).base_url_override.as_deref(),
        Some("https://one.example")
    );

    tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": { "base_url_override": null, "notes": null }
        }),
    )
    .await
    .expect("clearing the fields must be a supported write, not an error");

    let d = detail_of(&st, &id);
    assert_eq!(
        d.base_url_override, None,
        "null must clear, not be rejected"
    );
    assert_eq!(d.notes, None);
}

/// 字段缺席必须仍表示「保持原值」；正是这条性质让解析失败时的 `.ok()` 看起来无害。
#[tokio::test]
async fn an_omitted_field_is_left_unchanged() {
    let (st, id) = fixture();
    tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": { "url": "/v2/only-the-url" }
        }),
    )
    .await
    .expect("patch must apply");
    let d = detail_of(&st, &id);
    assert_eq!(d.url, "/v2/only-the-url");
    assert_eq!(d.method, HttpMethod::Get, "method must be untouched");
    assert!(d.params.is_empty(), "params must be untouched");
}

/// 经 `endpoint_get` 往返后，客户端给的 `id` 必须原样保留：agent 才能对表头列表做
/// 读—改—写，而不让旧行变成孤儿。
#[tokio::test]
async fn a_supplied_row_id_is_preserved() {
    let (st, id) = fixture();
    tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": {
                "headers": [{ "key": "A", "value": "1", "id": "row-stable-id" }]
            }
        }),
    )
    .await
    .expect("patch must apply");
    assert_eq!(detail_of(&st, &id).headers[0].id, "row-stable-id");
}

// =========================================================================
// 4. 端点 id 唯一性
// =========================================================================

/// `endpoint_create` 的 id 取自毫秒时钟，连发两个创建就是撞主键最可靠的复现方式；
/// 本用例要求两次创建拿到不同的 id，且都保留 `ep_` 前缀。
#[tokio::test]
async fn two_endpoint_creates_in_the_same_millisecond_get_distinct_ids() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_id", "I"))?;
        nodes::create(
            c,
            &node("p_id", "m_id", ResourceNodeType::Module, None),
            None,
        )?;
        permissions::set(c, "p_id", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    let mut ids = Vec::new();
    for i in 0..8 {
        let res = tool_endpoint_create(
            &st,
            serde_json::json!({
                "module_id": "m_id",
                "name": format!("e{i}"),
                "method": "GET",
                "url": "/x"
            }),
        )
        .await
        .unwrap_or_else(|e| panic!("create #{i} must succeed, got {e}"));
        let crate::mcp::CallToolContent::Text { text } = &res.content[0];
        let created: serde_json::Value = serde_json::from_str(text).expect("json result");
        ids.push(created["id"].as_str().expect("id").to_string());
    }

    let unique: std::collections::HashSet<&String> = ids.iter().collect();
    assert_eq!(
        unique.len(),
        ids.len(),
        "every create must get its own id; got {ids:?}"
    );
    // `ep_` 前缀是同级创建工具遵循的约定的一部分，所以放宽 id 也不能丢掉它。

    assert!(
        ids.iter().all(|id| id.starts_with("ep_")),
        "endpoint ids must keep the ep_ prefix; got {ids:?}"
    );
}

// =========================================================================
// 5. `endpoint_create` 一次调用即可接受完整配置
// =========================================================================

/// `body_mode` 只在 `parse_body_mode` 的表里逐个字面量列着，所以 multipart 存得进
/// `BodyMode` 并不等于 agent 写得进来。这里断言的是**往返**：经 `endpoint_create`
/// 写入后从存储行读回来仍是 multipart——只测解析成功会让一个写进去却退化成别的
/// 模式（于是发出去的 body 与建它的那个请求不同）的缺陷看起来是绿的。
#[tokio::test]
async fn endpoint_create_round_trips_a_multipart_body_mode() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_mp", "MP"))?;
        nodes::create(
            c,
            &node("p_mp", "m_mp", ResourceNodeType::Module, None),
            None,
        )?;
        permissions::set(c, "p_mp", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    // 分片数组的 JSON 文本就是 body_content —— 与其余 mode 共用同一个字段
    let content = r#"[{"name":"file","value":"BINARY","filename":"a.txt"}]"#;

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_mp",
            "name": "upload",
            "method": "POST",
            "url": "/api/upload",
            "body_mode": "multipart",
            "body_content": content,
        }),
    )
    .await
    .unwrap_or_else(|e| panic!("a multipart body_mode must be accepted, got {e}"));

    let crate::mcp::CallToolContent::Text { text } = &res.content[0];
    let created: serde_json::Value = serde_json::from_str(text).expect("json result");
    let id = created["id"].as_str().expect("id");

    let d = detail_of(&st, id);
    assert_eq!(d.body.mode, BodyMode::Multipart);
    assert_eq!(d.body.content, content);
}

/// `endpoint_update` 走 `RequestBody` 的 serde 反序列化而非 `parse_body_mode`，所以
/// multipart 能被 patch 是加变体的直接后果。断言一次，免得日后有人改 patch 读取器
/// 时把这条悄悄弄丢。
#[tokio::test]
async fn endpoint_update_can_patch_a_body_into_multipart() {
    let (st, id) = fixture();

    tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": { "body": { "mode": "multipart", "content": "[]" } }
        }),
    )
    .await
    .expect("a multipart body patch must be accepted");

    assert_eq!(detail_of(&st, &id).body.mode, BodyMode::Multipart);
}

/// `endpoint_create` 必须一次收下 `base_url_override`、`body_schema`、`docs`、
/// `notes` 与 `description`：这些字段若只有 `endpoint_update` 能写，搭一个配好的端点
/// 就得以两次调用收尾。
#[tokio::test]
async fn endpoint_create_accepts_the_full_configuration_in_one_call() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_c", "C"))?;
        nodes::create(c, &node("p_c", "m_c", ResourceNodeType::Module, None), None)?;
        permissions::set(c, "p_c", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_c",
            "name": "list orders",
            "description": "Orders for the current user",
            "method": "POST",
            "url": "/api/orders",
            "params": [{ "key": "page", "value": "1" }],
            "headers": [{ "key": "Accept", "value": "application/json" }],
            "body_mode": "json",
            "body_content": "{\"limit\":10}",
            "body_schema": "{\"limit\":\"number\"}",
            "auth_type": "bearer",
            "auth_config": { "bearer": "tok" },
            "tags": ["orders"],
            "base_url_override": "https://api.example.com",
            "docs": "Lists orders.",
            "notes": "n"
        }),
    )
    .await
    .expect("a fully specified create must succeed");

    let crate::mcp::CallToolContent::Text { text } = &res.content[0];
    let created: serde_json::Value = serde_json::from_str(text).expect("json result");
    let id = created["id"].as_str().expect("id").to_string();

    let d = detail_of(&st, &id);
    assert_eq!(d.method, HttpMethod::Post);
    assert_eq!(d.params.len(), 1);
    assert!(d.params[0].enabled);
    assert_eq!(d.headers.len(), 1);
    assert_eq!(d.body.mode, BodyMode::Json);
    assert_eq!(d.body.schema.as_deref(), Some("{\"limit\":\"number\"}"));
    assert_eq!(d.auth.kind, simple_post_core::domain::AuthType::Bearer);
    assert_eq!(d.auth.bearer.as_deref(), Some("tok"));
    assert_eq!(
        d.base_url_override.as_deref(),
        Some("https://api.example.com")
    );
    assert_eq!(d.docs.as_deref(), Some("Lists orders."));
    assert_eq!(d.notes.as_deref(), Some("n"));

    let n = simple_post_core::db::with_conn(&st.pool, |c| nodes::get(c, &id)).expect("node");
    assert_eq!(
        n.description.as_deref(),
        Some("Orders for the current user")
    );
}

/// `auth_config` 写错时不许被 `unwrap_or_default()` 吞成全默认的 `AuthConfig`：
/// 那样创建照样成功，却描述出一个 bearer token 根本不存在的端点。
#[tokio::test]
async fn endpoint_create_rejects_a_malformed_auth_config() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_ac", "AC"))?;
        nodes::create(
            c,
            &node("p_ac", "m_ac", ResourceNodeType::Module, None),
            None,
        )?;
        permissions::set(c, "p_ac", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ac",
            "name": "n",
            "method": "GET",
            "url": "/x",
            "auth_type": "bearer",
            "auth_config": { "bearer": { "tok": 1 } }
        }),
    )
    .await;
    assert!(
        matches!(res, Err(AppError::Validation(_))),
        "a malformed auth_config must be rejected, got {res:?}"
    );
}

/// 载荷与定义写反了：`body_content` 里放的是一份 JSON Schema。
///
/// 写错的代价是每次调试都把一份 Schema 当请求体发出去，而接口的文档里空无一物。
/// 两份内容都是 JSON 对象，凭内容分不出来；但顶层同时有 `type` 与 `properties` 时，
/// 几乎只有一种解释：这份是定义。
#[tokio::test]
async fn endpoint_create_rejects_a_schema_written_into_the_payload_slot() {
    let (st, _) = fixture();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "name": "n",
            "method": "POST",
            "url": "/api/users",
            "body_mode": "json",
            "body_content": "{\"type\":\"object\",\"properties\":{\"id\":{\"type\":\"string\"}}}"
        }),
    )
    .await;

    let Err(AppError::Validation(message)) = res else {
        panic!("a schema written into the payload slot must be rejected, got {res:?}");
    };
    assert!(
        message.contains("body_schema"),
        "the message must name the field the definition belongs in: {message}"
    );
}

/// 确实要把一份 Schema 当载荷发时，把同样的内容也写进 `body_schema` 即可 ——
/// 那时这份内容是有意选定的，不是写反了。
#[tokio::test]
async fn endpoint_create_accepts_a_schema_payload_when_the_schema_is_declared() {
    let (st, _) = fixture();
    const SCHEMA: &str = "{\"type\":\"object\",\"properties\":{\"id\":{\"type\":\"string\"}}}";

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "name": "n",
            "method": "POST",
            "url": "/api/schemas",
            "body_mode": "json",
            "body_content": SCHEMA,
            "body_schema": SCHEMA
        }),
    )
    .await
    .expect("a declared schema payload must be accepted");

    let crate::mcp::CallToolContent::Text { text } = &res.content[0];
    let created: serde_json::Value = serde_json::from_str(text).expect("json result");
    let id = created["id"].as_str().expect("id").to_string();
    assert_eq!(detail_of(&st, &id).body.schema.as_deref(), Some(SCHEMA));
}

/// patch 里写反了同样要拦：agent 常常先建一个空端点，再用 `endpoint_update` 填 body。
#[tokio::test]
async fn endpoint_update_rejects_a_schema_written_into_the_payload_slot() {
    let (st, id) = fixture();

    let res = tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": {
                "body": {
                    "mode": "json",
                    "content": "{\"type\":\"object\",\"properties\":{\"id\":{\"type\":\"string\"}}}"
                }
            }
        }),
    )
    .await;

    assert!(
        matches!(res, Err(AppError::Validation(_))),
        "a schema written into the payload slot must be rejected, got {res:?}"
    );
}

/// 只带 `type` 的载荷不能被拦下来：`{"type":"order"}` 是很普通的业务数据。
#[tokio::test]
async fn a_payload_with_only_a_type_key_is_accepted() {
    let (st, _) = fixture();

    tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "name": "n",
            "method": "POST",
            "url": "/api/orders",
            "body_mode": "json",
            "body_content": "{\"type\":\"order\"}"
        }),
    )
    .await
    .expect("an ordinary payload must not be rejected");
}

/// 端点自己的 HTTPS 出站设置要能一次建好，不必再补一次 `endpoint_update`。
///
/// 键名是 camelCase（`skipVerify` / `caCertificatePath`），与 `TlsSettings` 自己的
/// serde 键名一致 —— 故这份载荷直接反序列化成那个类型。
#[tokio::test]
async fn endpoint_create_stores_the_tls_settings_in_one_call() {
    let (st, _) = fixture();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "name": "mtls endpoint",
            "method": "GET",
            "url": "/api/secure",
            "tls": {
                "skipVerify": true,
                "caCertificatePath": "/etc/ca.pem",
                "clientCertificatePath": "/etc/client.pem",
                "clientKeyPath": "/etc/client.key"
            }
        }),
    )
    .await
    .expect("a create carrying tls settings must succeed");

    let crate::mcp::CallToolContent::Text { text } = &res.content[0];
    let created: serde_json::Value = serde_json::from_str(text).expect("json result");
    let id = created["id"].as_str().expect("id").to_string();

    let stored = detail_of(&st, &id)
        .tls
        .expect("端点自带的 HTTPS 设置应当落库");
    assert!(stored.skip_verify);
    assert_eq!(stored.ca_certificate_path.as_deref(), Some("/etc/ca.pem"));
    assert_eq!(
        stored.client_certificate_path.as_deref(),
        Some("/etc/client.pem")
    );
    assert_eq!(stored.client_key_path.as_deref(), Some("/etc/client.key"));
}

/// 不给 `tls` 的端点继承全局默认：存储行里那一列是空的。
#[tokio::test]
async fn endpoint_create_without_tls_leaves_the_column_empty() {
    let (st, _) = fixture();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "name": "plain endpoint",
            "method": "GET",
            "url": "/api/plain"
        }),
    )
    .await
    .expect("create must succeed");

    let crate::mcp::CallToolContent::Text { text } = &res.content[0];
    let created: serde_json::Value = serde_json::from_str(text).expect("json result");
    let id = created["id"].as_str().expect("id").to_string();
    assert!(
        detail_of(&st, &id).tls.is_none(),
        "没给 `tls` 的端点继承全局默认，存储行里那一列应当是空的"
    );
}

/// **键名拼错**的 `tls` 载荷必须报错，不能被静默丢掉。
///
/// 丢掉的后果比丢掉一个普通字段更重：整份设置会反序列化成「什么都没设」的值 ——
/// 而端点自带的那份**整体**取代全局默认，于是全局的「跳过校验」与 CA 路径在这个端点
/// 上全部失效。调用方却以为跳过校验已经打开。
#[tokio::test]
async fn endpoint_create_rejects_a_misspelled_tls_key() {
    let (st, _) = fixture();
    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "name": "bad tls endpoint",
            "method": "GET",
            "url": "/api/bad-tls",
            // `skip_verify` 是**发送工具**那个对象里的键名；写工具这一侧用 camelCase。
            "tls": { "skip_verify": true }
        }),
    )
    .await;
    let Err(err) = res else {
        panic!("expected the create to fail, got {res:?}");
    };
    let msg = err.to_string();
    assert!(
        msg.contains("skip_verify"),
        "报错要点名那个不认识的键，got: {msg}"
    );
}

/// TLS 载荷里粘了证书正文时，**树上不能留下集合**。
///
/// 路径上的集合是逐级建出来的，每一级都单独提交。若 TLS 的校验留到写
/// `endpoint_details` 那一行才做（那里也校验），那么「集合建好了、TLS 被拒、端点没
/// 建成」就会留下一串没人要的集合。故 TLS 要在建集合之前就校验。
#[tokio::test]
async fn endpoint_create_rejects_a_pasted_certificate_body_without_leaving_collections_behind() {
    let (st, _) = fixture();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "collection_path": "订单/明细",
            "name": "bad tls endpoint",
            "method": "GET",
            "url": "/api/bad-tls",
            "tls": {
                "caCertificatePath":
                    "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----"
            }
        }),
    )
    .await;

    let Err(err) = res else {
        panic!("a pasted certificate body must be rejected, got {res:?}");
    };
    let msg = err.to_string();
    assert!(
        msg.contains("自定义 CA 证书"),
        "报错要点名是哪一个字段，got: {msg}"
    );

    let collections = simple_post_core::db::with_conn(&st.pool, |c| {
        nodes::list_by_root_and_type(c, "p_ep_fix", ResourceNodeType::Collection)
    })
    .expect("list collections");
    let names: Vec<&str> = collections.iter().map(|c| c.name.as_str()).collect();
    assert!(
        !names.contains(&"订单") && !names.contains(&"明细"),
        "校验失败不许在树上留下集合，got: {names:?}"
    );
}

/// 响应定义不合法时必须在**建集合之前**就报错：否则「集合建好了、定义被拒、端点没建成」
/// 会留下一串没人要的集合与一份打不开的树。
#[tokio::test]
async fn endpoint_create_rejects_a_bad_response_definition_before_writing_anything() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_rj", "RJ"))?;
        nodes::create(
            c,
            &node("p_rj", "m_rj", ResourceNodeType::Module, None),
            None,
        )?;
        permissions::set(c, "p_rj", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_rj",
            "collection_path": "订单/明细",
            "name": "n",
            "method": "GET",
            "url": "/x",
            "responses": [
                { "status": "200", "description": "成" },
                { "status": "2OO", "description": "键写坏了" },
                { "status": "200", "description": "重复" },
                { "status": "201", "description": "x", "schema": "not an object" },
                { "status": "202", "description": "x", "contentType": "json" }
            ]
        }),
    )
    .await;
    let message = match res {
        Err(AppError::Validation(message)) => message,
        other => panic!("expected a validation error, got {other:?}"),
    };
    assert!(message.contains("responses[1].status"), "got {message}");
    assert!(message.contains("responses[2].status"), "got {message}");
    assert!(message.contains("responses[3].schema"), "got {message}");
    assert!(
        message.contains("responses[4].contentType"),
        "got {message}"
    );

    // `collection_path` 上的两级集合一个都不该留下来。
    let children =
        simple_post_core::db::with_conn(&st.pool, |c| nodes::list_children(c, "m_rj", None))
            .expect("children");
    assert!(
        children.is_empty(),
        "a rejected create must not leave collections behind: {children:?}"
    );
}

#[tokio::test]
async fn endpoint_create_mints_ids_for_response_definitions() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_rs", "RS"))?;
        nodes::create(
            c,
            &node("p_rs", "m_rs", ResourceNodeType::Module, None),
            None,
        )?;
        permissions::set(c, "p_rs", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_rs",
            "name": "n",
            "method": "GET",
            "url": "/x",
            "responses": [
                { "status": "2XX", "description": "成", "contentType": "application/json",
                  "schema": { "type": "object" }, "example": "{\"id\":1}" },
                { "id": "resp_kept", "status": "default", "description": "兜底" }
            ]
        }),
    )
    .await
    .expect("definitions that pass the rules must be stored");

    let crate::mcp::CallToolContent::Text { text } = &res.content[0];
    let created: serde_json::Value = serde_json::from_str(text).expect("json result");
    let id = created["id"].as_str().expect("id").to_string();

    let d = detail_of(&st, &id);
    assert_eq!(d.responses.len(), 2);
    // 范围与 `default` 都收；缺 id 的那条铸成 `resp_` 前缀。
    assert_eq!(d.responses[0].status, "2XX");
    assert!(
        d.responses[0].id.starts_with("resp_"),
        "got {}",
        d.responses[0].id
    );
    assert_eq!(d.responses[1].id, "resp_kept");
    assert_eq!(d.responses[0].content_type, "application/json");
    assert_eq!(d.responses[0].example.as_deref(), Some("{\"id\":1}"));
}
