//! 导入 / 导出四个端点的**线格式**：请求体就是渲染层发出的那个扁平 camelCase 对象。
//!
//! 驱动真实 listener，理由同 `response_history_test.rs`：axum 的 `Json` 提取器在 body
//! 对不上字段时回的是 4xx 而不是 panic，这属于路由与 DTO 的性质，读 handler 读不出来。
//!
//! 这一层要有行为级覆盖：`interchange_api_test.rs` 直接调
//! `crate::interchange_api::import_preview`，body 的形状在它上面根本没经过任何反序列化。
//! 于是「Tauri 侧把 payload 包进 `opts`」这类改法在浏览器侧毫无症状——而它同时会让桌面
//! 侧全挂。桌面那半由 `src-tauri/src/commands/interchange_test.rs` 从源码形状上钉住，
//! 这一半由这里的真实请求钉住。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::web_api::{build_router, WebState};

/// 一份能被 `import::sniff` 认成 Postman v2.1 的最小集合（它只看顶层 `item` + `info`）。
const POSTMAN: &str = r#"{
  "info": { "name": "demo", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
  "item": [
    {
      "name": "users",
      "item": [
        {
          "name": "list users",
          "request": { "method": "GET", "url": { "raw": "/users", "host": ["{{baseUrl}}"], "path": ["users"] } }
        },
        {
          "name": "create user",
          "request": { "method": "POST", "url": { "raw": "/users", "host": ["{{baseUrl}}"], "path": ["users"] }, "body": { "mode": "raw", "raw": "{}" } }
        }
      ]
    }
  ]
}"#;

/// 同一份集合，但 host 写成了字面量。`{{baseUrl}}` 那种变量 host 拆不出地址，
/// 所以这份才带得出地址。
const POSTMAN_WITH_HOST: &str = r#"{
  "info": { "name": "demo", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
  "item": [
    {
      "name": "users",
      "item": [
        {
          "name": "list users",
          "request": {
            "method": "GET",
            "url": {
              "raw": "https://api.example.com/users",
              "protocol": "https",
              "host": ["api", "example", "com"],
              "path": ["users"]
            }
          }
        }
      ]
    }
  ]
}"#;

/// 同一份集合的单行压缩形态。CDN 上的 spec 与 `JSON.stringify` 的产物都是这个形状，
/// 格式识别必须认得出它。
const POSTMAN_COMPRESSED: &str = r#"{"info":{"name":"demo","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":[{"name":"users","item":[{"name":"list users","request":{"method":"GET","url":{"raw":"/users","host":["{{baseUrl}}"],"path":["users"]}}}]}]}"#;

/// 建一个 `WebState`：一次性的 SQLite 文件 + 空 SPA 目录，router 绑在 loopback 上一个
/// 临时端口。
///
/// 从 `response_history_test.rs` 复制而来而非共用，理由同那里：计数器是每模块各自的，
/// 而跨两个 `#[path]` 测试模块共享一套夹具需要一个谁都不拥有的 `pub(crate)` 落点。
async fn serve_test_router() -> (SocketAddr, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let dir = std::env::temp_dir().join(format!(
        "simple-post-interchange-routes-{}-{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");

    let spa_dir = dir.join("spa");
    std::fs::create_dir_all(&spa_dir).expect("spa dir");

    let pool = crate::db::init(&dir).expect("db init");
    seed_tree(&dir);
    let state = WebState {
        pool: pool.clone(),
        daemon: crate::daemon_supervisor::McpDaemonHandle::new(),
        data_dir: dir.clone(),
        mcp_state: Arc::new(crate::mcp::AppState {
            pool,
            started_at: std::time::Instant::now(),
            metrics: Arc::new(crate::mcp_metrics::McpMetrics::new()),
        }),
        password: None,
        base_path: None,
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let router = build_router(state, spa_dir);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (addr, dir)
}

/// 落点要真实存在：导入挂在 `mod_a` 下，导出也从 `mod_a` 读。种一个项目 + 一个模块就够，
/// endpoint 由上面的 commit 现场造出来。
///
/// 另有一个默认环境：地址住在（环境 × 模块）那一格，没有默认环境就没有格子可填。
fn seed_tree(dir: &Path) {
    let pool = crate::db::init(dir).expect("db init");
    crate::db::with_tx(&pool, |c| {
        c.execute_batch(
            "INSERT INTO projects (id, name, color, created_at, updated_at) \
             VALUES ('p1', 'demo', '#000', 1, 1)",
        )?;
        c.execute_batch(
            "INSERT INTO nodes (id, root_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('mod_a', 'p1', 'module', 'A', 0, 1, 1)",
        )?;
        crate::db::environments::create(
            c,
            &crate::domain::Environment {
                id: "env_a".to_string(),
                project_id: "p1".to_string(),
                name: "开发".to_string(),
                module_base_urls: Default::default(),
                variables: Default::default(),
                disabled_variables: Vec::new(),
                created_at: 0,
            },
        )?;
        Ok(())
    })
    .expect("seed tree");
}

async fn post_json(addr: SocketAddr, path: &str, body: Value) -> (reqwest::StatusCode, Value) {
    let res = reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .json(&body)
        .send()
        .await
        .expect("request completes");
    let status = res.status();
    let body = res.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn get_json(addr: SocketAddr, path: &str) -> (reqwest::StatusCode, Value) {
    let res = reqwest::Client::new()
        .get(format!("http://{addr}{path}"))
        .send()
        .await
        .expect("request completes");
    let status = res.status();
    let body = res.json().await.unwrap_or(Value::Null);
    (status, body)
}

/// 勾选树里的全部端点 key。空集是合法输入，故这里递归取。
fn endpoint_keys(node: &Value, out: &mut Vec<String>) {
    for ep in node["endpoints"].as_array().into_iter().flatten() {
        out.push(ep["key"].as_str().expect("endpoint key").to_string());
    }
    for child in node["children"].as_array().into_iter().flatten() {
        endpoint_keys(child, out);
    }
}

/// 渲染层 `useTransfer` 逐字发出的四个 body。**不要**在这里"整理"它们：这份测试的全部
/// 意义就是「服务端吃的就是这些键」。
async fn round_trip(addr: SocketAddr) -> (Value, Value) {
    let (status, preview) = post_json(
        addr,
        "/api/interchange/import/preview",
        json!({ "projectId": "p1", "moduleId": "mod_a", "content": POSTMAN }),
    )
    .await;
    assert_eq!(
        status, 200,
        "import preview rejected the flat body: {preview}"
    );

    let mut keys = Vec::new();
    for group in preview["groups"].as_array().into_iter().flatten() {
        endpoint_keys(group, &mut keys);
    }
    assert!(
        !keys.is_empty(),
        "the fixture must yield selectable endpoints"
    );

    let (status, committed) = post_json(
        addr,
        "/api/interchange/import/commit",
        json!({
            "projectId": "p1",
            "moduleId": "mod_a",
            "content": POSTMAN,
            "selected": keys,
        }),
    )
    .await;
    assert_eq!(
        status, 200,
        "import commit rejected the flat body: {committed}"
    );
    assert_eq!(
        committed["createdEndpoints"].as_u64(),
        Some(keys.len() as u64),
        "every selected key must land: {committed}"
    );

    let (status, export_preview) = post_json(
        addr,
        "/api/interchange/export/preview",
        json!({ "projectId": "p1", "moduleId": "mod_a" }),
    )
    .await;
    assert_eq!(
        status, 200,
        "export preview rejected the flat body: {export_preview}"
    );

    let mut export_keys = Vec::new();
    for group in export_preview["groups"].as_array().into_iter().flatten() {
        endpoint_keys(group, &mut export_keys);
    }
    let (status, document) = post_json(
        addr,
        "/api/interchange/export/document",
        json!({
            "projectId": "p1",
            "moduleId": "mod_a",
            "environmentId": "",
            "selected": export_keys,
            "format": "yaml",
        }),
    )
    .await;
    assert_eq!(
        status, 200,
        "export document rejected the flat body: {document}"
    );
    (preview, document)
}

#[tokio::test]
async fn the_four_endpoints_take_the_body_the_renderer_sends() {
    let (addr, dir) = serve_test_router().await;
    let (preview, document) = round_trip(addr).await;

    assert_eq!(preview["format"], "postman-collection-v2.1");
    assert_eq!(preview["counts"]["endpoints"].as_u64(), Some(2));
    assert_eq!(
        document["mediaType"], "application/yaml",
        "`format: \"yaml\"` must reach the exporter"
    );
    assert!(
        document["content"]
            .as_str()
            .unwrap_or_default()
            .contains("openapi:"),
        "the produced document must be the YAML the renderer asked for"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn an_optional_key_may_be_absent_entirely() {
    // URL 模式下渲染层根本不发 `content`。它必须被当成「没给 content」而不是反序列化
    // 失败——两条路都会给一个 4xx，区别只在于是哪一条。
    let (addr, dir) = serve_test_router().await;
    let (status, body) = post_json(
        addr,
        "/api/interchange/import/preview",
        // 既没有 content 也没有 url：真实的错误信息才是这次要断言的东西。
        json!({ "projectId": "p1", "moduleId": "mod_a" }),
    )
    .await;
    assert_eq!(
        status, 400,
        "a missing `content` must reach the handler and be reported by it, \
         not rejected by the extractor: {body}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_wrapped_opts_body_is_rejected() {
    // 反面：把整个 opts 包进一个 `opts` 键能修好桌面侧，同时悄悄弄坏浏览器侧。
    // 这个端点必须拒绝它，否则两侧发的不再是同一份字节。
    //
    // 断言的是 **422**（axum 的 `Json` 提取器反序列化失败），不是笼统的 4xx：`content`
    // 与 `moduleId` 都缺失时，handler 自己也会回 400（见上面那条）。写「4xx」的话，
    // 这条测试在提取器被改成宽松时依然会绿——那正是它本该抓的改动。
    let (addr, dir) = serve_test_router().await;
    let (status, body) = post_json(
        addr,
        "/api/interchange/import/preview",
        json!({
            "opts": { "projectId": "p1", "moduleId": "mod_a", "content": POSTMAN },
        }),
    )
    .await;
    assert_eq!(
        status, 422,
        "the wire shape is the flat body; a wrapped one must be rejected by the \
         extractor itself, not reach the handler: got {status}: {body}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_compressed_collection_is_accepted() {
    // 单行压缩过的 JSON 认不出来 = 导入完全不可用。这是格式识别的第一道门，
    // 走的是真实的 HTTP 路径。
    let (addr, dir) = serve_test_router().await;
    let (status, preview) = post_json(
        addr,
        "/api/interchange/import/preview",
        json!({ "projectId": "p1", "moduleId": "mod_a", "content": POSTMAN_COMPRESSED }),
    )
    .await;
    assert_eq!(status, 200, "{preview}");
    assert_eq!(preview["format"], "postman-collection-v2.1");

    let mut keys = Vec::new();
    for group in preview["groups"].as_array().into_iter().flatten() {
        endpoint_keys(group, &mut keys);
    }
    assert!(!keys.is_empty(), "压缩形态的集合也必须选出端点: {preview}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_literal_host_becomes_a_base_url_and_apply_base_url_defaults_to_true() {
    // 这条盯的是**线上的默认值**：`applyBaseUrl` 缺席必须等于真。缺席若等于假，
    // 用户在预览里看到的「将写入地址」到落库时就消失了，而两边都是 200。
    let (addr, dir) = serve_test_router().await;
    let (status, preview) = post_json(
        addr,
        "/api/interchange/import/preview",
        json!({ "projectId": "p1", "moduleId": "mod_a", "content": POSTMAN_WITH_HOST }),
    )
    .await;
    assert_eq!(status, 200, "{preview}");
    assert_eq!(preview["baseUrls"][0]["url"], "https://api.example.com");
    assert_eq!(preview["baseUrls"][0]["environmentName"], "开发");

    let mut keys = Vec::new();
    for group in preview["groups"].as_array().into_iter().flatten() {
        endpoint_keys(group, &mut keys);
    }
    let (status, committed) = post_json(
        addr,
        "/api/interchange/import/commit",
        json!({
            "projectId": "p1",
            "moduleId": "mod_a",
            "content": POSTMAN_WITH_HOST,
            "selected": keys,
        }),
    )
    .await;
    assert_eq!(status, 200, "{committed}");
    assert_eq!(committed["baseUrls"][0]["url"], "https://api.example.com");
    assert_eq!(committed["baseUrls"][0]["moduleId"], "mod_a");
    assert_eq!(committed["baseUrls"][0]["environmentName"], "开发");

    // 光看报告不够——地址得真的落进那一格。
    let (status, envs) = get_json(addr, "/api/environments?projectId=p1").await;
    assert_eq!(status, 200, "{envs}");
    assert_eq!(
        envs[0]["moduleBaseUrls"]["mod_a"]["url"], "https://api.example.com",
        "地址得写进 (环境 × 模块) 那一格: {envs}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn apply_base_url_false_leaves_the_cell_alone() {
    let (addr, dir) = serve_test_router().await;
    let (status, preview) = post_json(
        addr,
        "/api/interchange/import/preview",
        json!({ "projectId": "p1", "moduleId": "mod_a", "content": POSTMAN_WITH_HOST }),
    )
    .await;
    assert_eq!(status, 200, "{preview}");
    let mut keys = Vec::new();
    for group in preview["groups"].as_array().into_iter().flatten() {
        endpoint_keys(group, &mut keys);
    }

    let (status, committed) = post_json(
        addr,
        "/api/interchange/import/commit",
        json!({
            "projectId": "p1",
            "moduleId": "mod_a",
            "content": POSTMAN_WITH_HOST,
            "selected": keys,
            "applyBaseUrl": false,
        }),
    )
    .await;
    assert_eq!(status, 200, "{committed}");
    assert!(
        committed.get("baseUrls").is_none(),
        "关掉之后报告里不该有地址: {committed}"
    );
    let (status, envs) = get_json(addr, "/api/environments?projectId=p1").await;
    assert_eq!(status, 200, "{envs}");
    assert!(
        envs[0]["moduleBaseUrls"]
            .as_object()
            .is_some_and(|m| m.is_empty()),
        "关掉之后那一格必须没被写过: {envs}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 一份超过 axum 默认 2 MB 上限的集合。
///
/// **靠体积顶上去，不靠 operation 数**——这里要验的是路由与请求体上限，不是解析器的吞吐，
/// 所以 operation 保持两位数，体积靠长 description 撑。
fn big_postman() -> Value {
    let pad = "x".repeat(20_000);
    let items: Vec<Value> = (0..120)
        .map(|i| {
            json!({
                "name": format!("ep{i}"),
                "request": {
                    "method": "GET",
                    "url": { "path": ["users", i.to_string()] },
                    "description": pad,
                }
            })
        })
        .collect();
    json!({
        "info": { "name": "big", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
        "item": items,
    })
}

/// 一份比 axum 默认上限大得多的文档必须真的进得来。
///
/// 这条钉的是**部署形态之间的一致性**：这四个端点的请求体装的是整份文档，而
/// `DefaultBodyLimit` 不给就是 2 MB。少了那层，超大文档在浏览器 / Docker 形态回 413
/// （传输层错误，读不出「文档太大」），桌面 Tauri 形态同一份文件却能导入——两边都不
/// 报错，只有用户看得见。
///
/// 与 `interchange_api_test.rs` 里那份 3 MB 用例互补：那份直接调函数，根本不过 HTTP。
#[tokio::test]
async fn a_document_far_over_the_default_body_limit_still_reaches_the_handler() {
    let (addr, dir) = serve_test_router().await;
    let doc = big_postman();
    // 缩进打印：格式嗅探只看顶层键所在的**行首**，压成单行它就认不出来了
    // （那是 #70，单行压缩 JSON 认不出，与这条测试要钉的上限无关）。
    let doc_text = serde_json::to_string_pretty(&doc).expect("serialise");
    assert!(
        doc_text.len() > 2 * 1024 * 1024,
        "这份集合必须真的超过 axum 的 2 MB 默认值，否则这条测试什么也没钉住：{} 字节",
        doc_text.len()
    );

    let (status, preview) = post_json(
        addr,
        "/api/interchange/import/preview",
        json!({ "projectId": "p1", "moduleId": "mod_a", "content": doc_text }),
    )
    .await;
    assert_eq!(
        status, 200,
        "浏览器形态必须能吃下与桌面同样大的文档：{preview}"
    );
    assert!(
        preview["groups"].is_array(),
        "拿到的是预览而不是 413 的错误体：{preview}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
