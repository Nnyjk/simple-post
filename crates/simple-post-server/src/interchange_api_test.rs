//! 编排层：内容来源、IR → 线形状的投影、文件名、往返，以及**真的发一次 HTTP 请求**。

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{
    EndpointDetail, HttpMethod, Project, ResourceNode, ResourceNodeType,
};
use simple_post_core::error::AppError;
use simple_post_interchange::fingerprint::FingerprintSet;
use simple_post_interchange::ir::{Exchange, IrEndpoint, IrModule};
use simple_post_interchange::Diagnostic;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::dto::{ImportCommitOpts, ImportPreviewOpts, TransferNode};

use super::{
    all_endpoints, count_groups, diagnostics_of, file_name, import_commit, import_preview,
    mark_duplicates, project_modules, FETCH_MAX_KB,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn ep(key: &str, path: &str) -> IrEndpoint {
    IrEndpoint {
        key: key.into(),
        ..IrEndpoint::new(key, HttpMethod::Get, path)
    }
}

fn exchange() -> Exchange {
    Exchange {
        project_name: "示例".into(),
        modules: vec![IrModule {
            key: "/item/0".into(),
            name: "用户".into(),
            endpoints: vec![
                ep("/item/0/request/0", "/users"),
                ep("/item/0/request/1", "/users/{id}"),
            ],
            ..Default::default()
        }],
        diagnostics: vec![
            // 落在第一个端点上：pointer 正好等于它的 key。
            Diagnostic::warn("/item/0/request/0", "url 里的 host 是 {{baseUrl}}"),
            // 也落在它上面：这是 key 的子路径。
            Diagnostic::info("/item/0/request/0/body", "表单里的 file 字段被丢掉"),
            // 落在第二个端点上。
            Diagnostic::warn("/item/0/request/1", "auth 是 apikey in cookie"),
            // 不属于任何一行：文档级的说明。
            Diagnostic::info("/info", "集合没有 variable"),
        ],
        ..Default::default()
    }
}

#[test]
fn a_diagnostic_lands_on_the_endpoint_its_pointer_points_into() {
    let ex = exchange();
    assert_eq!(
        diagnostics_of(&ex, "/item/0/request/0"),
        vec![0, 1],
        "pointer 命中自身与子路径都算落在这一行"
    );
    assert_eq!(diagnostics_of(&ex, "/item/0/request/1"), vec![2]);
    // 文档级那条不归任何一个端点：把它的 pointer 当 key 去问，任何一行都领不走它。
    for endpoint in all_endpoints(&ex) {
        assert!(
            !diagnostics_of(&ex, &endpoint.key).contains(&3),
            "文档级诊断被 {key} 认领了",
            key = endpoint.key
        );
    }
}

#[test]
fn a_shorter_key_does_not_claim_a_longer_siblings_diagnostics() {
    // `/item/2` 与 `/item/21` 前缀相同，前缀比对必须补一个 `/` 才分得开。
    let ex = Exchange {
        diagnostics: vec![Diagnostic::warn("/item/21/request", "只属于 21")],
        ..Default::default()
    };
    assert!(diagnostics_of(&ex, "/item/2").is_empty());
    assert_eq!(diagnostics_of(&ex, "/item/21").len(), 1);
}

#[test]
fn duplicates_are_marked_without_booking_the_ledger() {
    let existing = [endpoint_detail("n1", HttpMethod::Get, "/users")];
    let mut set = FingerprintSet::new();
    set.absorb_details(existing.iter());

    let ex = exchange();
    let dup = mark_duplicates(&ex, &set);
    assert_eq!(dup.len(), 1, "只有 GET /users 那条命中");
    assert_eq!(dup, BTreeSet::from(["/item/0/request/0".to_string()]));

    // 关键：预览**不能**记账。用 `seen` 会把本次导入的条目先记一遍，于是落库时
    // 第一个端点被自己的预览当成重复而跳过——所以连问两次答案必须一样。
    assert_eq!(
        mark_duplicates(&ex, &set),
        dup,
        "预览只问不记：第二次问的答案不变"
    );
}

#[test]
fn the_wire_tree_counts_every_level_of_groups() {
    let ex = Exchange {
        modules: vec![IrModule {
            key: "m".into(),
            name: "m".into(),
            endpoints: vec![ep("a", "/a")],
            collections: vec![
                simple_post_interchange::ir::IrCollection {
                    key: "c0".into(),
                    name: "c0".into(),
                    endpoints: vec![ep("b", "/b")],
                    collections: vec![simple_post_interchange::ir::IrCollection {
                        key: "c0c0".into(),
                        name: "c0c0".into(),
                        endpoints: vec![ep("c", "/c")],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                simple_post_interchange::ir::IrCollection {
                    key: "c1".into(),
                    name: "c1".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    let groups = project_modules(&ex.modules, &BTreeSet::new(), &ex);
    // 1 顶层 + c0 + c0c0 + c1 = 4。含那个空的：空分组在预览里是真实信息——用户看得见
    // 「这个文件夹里什么都没导进来」。
    assert_eq!(count_groups(&groups), 4);
    assert_eq!(groups[0].endpoint_total, 3);
    assert_eq!(groups[0].children[1].endpoint_total, 0);
}

#[test]
fn a_file_name_cannot_escape_its_directory() {
    use simple_post_interchange::openapi::DocFormat;
    for raw in ["../../etc/passwd", "a/b\\c", "  ", "报告:v2*"] {
        let name = file_name(raw, DocFormat::Yaml);
        assert!(
            !name.contains('/') && !name.contains('\\') && !name.contains(':'),
            "文件名里不能有路径分隔符或盘符, got: {name}"
        );
        assert!(name.ends_with(".yaml"), "got: {name}");
    }
    assert_eq!(file_name("我的项目", DocFormat::Json), "我的项目.json");
    assert_eq!(file_name("   ", DocFormat::Json), "simple-post.json");
}

#[test]
fn a_project_name_that_is_only_punctuation_still_yields_a_name() {
    use simple_post_interchange::openapi::DocFormat;
    assert_eq!(file_name("///", DocFormat::Json), "---.json");
}

#[test]
fn all_endpoints_reaches_through_nested_collections() {
    // `mark_duplicates` 与计数都靠它。漏掉集合里的那一层不会让任何一步报错，
    // 只会让「项目里已经有了」的标记在嵌套分组上整个消失。
    let ex = Exchange {
        modules: vec![IrModule {
            key: "m".into(),
            name: "m".into(),
            endpoints: vec![ep("top", "/top")],
            collections: vec![simple_post_interchange::ir::IrCollection {
                key: "c".into(),
                name: "c".into(),
                endpoints: vec![ep("mid", "/mid")],
                collections: vec![simple_post_interchange::ir::IrCollection {
                    key: "cc".into(),
                    name: "cc".into(),
                    endpoints: vec![ep("deep", "/deep")],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut keys: Vec<&str> = all_endpoints(&ex).iter().map(|e| e.key.as_str()).collect();
    keys.sort();
    assert_eq!(keys, vec!["deep", "mid", "top"]);
}

#[test]
fn an_import_preview_never_books_the_fingerprint_ledger() {
    // 走真库的那一半：账本只灌**已落库**的行。若这里灌了别的东西（或者用了会记账的
    // `seen`），落库时第一个端点会被自己的预览当成重复而跳过。
    let pool = temp_pool("preview");
    let project_id = seed_one_endpoint(&pool);

    let ex = exchange();
    let (dup, again) = db::with_conn(&pool, |conn| {
        let existing = db::endpoint_details::list_by_root(conn, &project_id)?;
        let mut set = FingerprintSet::new();
        set.absorb_details(existing.iter());
        let dup = mark_duplicates(&ex, &set);
        // 预览跑完之后，同一批数据再问一次答案必须不变。
        let again = mark_duplicates(&ex, &set);
        Ok::<_, AppError>((dup, again))
    })
    .expect("mark");

    assert_eq!(
        dup,
        BTreeSet::from(["/item/0/request/0".to_string()]),
        "库里的 GET /users 应当被认出来"
    );
    assert_eq!(again, dup, "预览不记账：连问两次答案一样");
}

#[tokio::test]
async fn the_preview_counts_the_variables_the_source_disabled() {
    // `disabledVariableCount` 曾经是字面量 0：一个承诺了意义的公开契约字段，产不出那个数。
    let pool = temp_pool("disabled-vars");
    let project_id = seed_project_with_module(&pool).0;
    let content = r#"{
      "info": { "name": "demo" },
      "item": [],
      "variable": [
        { "key": "token", "value": "abc" },
        { "key": "secret", "value": "s3cr3t", "disabled": true }
      ]
    }"#;

    let preview = import_preview(
        &pool,
        &ImportPreviewOpts {
            project_id,
            module_id: "m1".into(),
            content: Some(content.into()),
            url: None,
        },
    )
    .await
    .expect("预览");

    assert_eq!(preview.environments.len(), 1);
    assert_eq!(preview.environments[0].variable_count, 2);
    assert_eq!(
        preview.environments[0].disabled_variable_count, 1,
        "源文档里被停用的变量要数出来"
    );
}

// --- URL 分支：真发一次请求 ------------------------------------------------
//
// #68：整条 URL 分支此前 0 个测试覆盖，而这些性质**只有真的发一次请求才会验出来**。
// 上限抬到了 8 MB、截断被拦下了，这些都是读代码得出的结论；「一份 3 MB 的文档
// 能不能真的走完拉取 → 解析 → 预览」不是。下面每条都对着本机 loopback 上的一台
// 临时服务器跑，故 CI 的 ubuntu 与 Windows 都不需要外网。

/// `import_preview` 自己调 [`simple_post_core::http_executor::OutboundPolicy::from_env`]，
/// 所以策略只能经环境变量改——而环境变量是**整个进程共享的一个槽位**。
///
/// 持锁是必须的，且本文件里**每一个发请求的测试都要走这里**：否则一个正在跑
/// `with(true)` 的测试会把另一个指望放行的测试一起拒掉。手法照 `port_registry_test.rs`。
static POLICY_LOCK: Mutex<()> = Mutex::new(());

const POLICY_ENV: &str = "SIMPLE_POST_BLOCK_PRIVATE_NET";

struct ScopedPolicy {
    _guard: MutexGuard<'static, ()>,
    prev: Option<std::ffi::OsString>,
}

impl ScopedPolicy {
    fn with(block_private_net: bool) -> Self {
        // 先拿锁，*再*换变量。顺序反了就会出现「换了变量、锁还没拿到」的那个窗口。
        let guard = POLICY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os(POLICY_ENV);
        // 显式写 `0` 而不只是删掉：本机若本来就设了这个变量，「放行」那几条也得成立。
        // SAFETY：锁保证没有别的线程看得见中间状态。
        unsafe { std::env::set_var(POLICY_ENV, if block_private_net { "1" } else { "0" }) };
        Self {
            _guard: guard,
            prev,
        }
    }
}

impl Drop for ScopedPolicy {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => unsafe { std::env::set_var(POLICY_ENV, v) },
            None => unsafe { std::env::remove_var(POLICY_ENV) },
        }
        // `_guard` 在本方法之后才 drop，把锁让给下一个测试。
    }
}

/// 一台只回固定 body 的 loopback 服务器，外加一个请求计数器。
///
/// 用裸 [`tokio::net::TcpListener`] 而不是 axum：这里要伺服的是**几 MB**的单个响应，
/// 路由与状态机都是多余的，而「它被请求了几次」正是本节要断言的东西之一。
struct DocServer {
    addr: SocketAddr,
    hits: Arc<AtomicU32>,
}

impl DocServer {
    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    fn hits(&self) -> u32 {
        self.hits.load(Ordering::Relaxed)
    }
}

async fn serve_doc(body: String) -> DocServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let hits = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&hits);
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::Relaxed);
            // 读完请求头就回。请求行是什么在这里不影响任何断言。
            let mut head = Vec::new();
            let mut buf = [0u8; 2048];
            loop {
                match sock.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        head.extend_from_slice(&buf[..n]);
                        if head.windows(4).any(|w| w == b"\r\n\r\n") || head.len() >= 2048 {
                            break;
                        }
                    }
                }
            }
            let mut out = Vec::with_capacity(body.len() + 256);
            out.extend_from_slice(b"HTTP/1.1 200 OK\r\nContent-Type: application/yaml\r\n");
            out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
            out.extend_from_slice(b"Connection: close\r\n\r\n");
            out.extend_from_slice(body.as_bytes());
            let _ = sock.write_all(&out).await;
            let _ = sock.shutdown().await;
        }
    });
    DocServer { addr, hits }
}

/// 一份**合法**的 OpenAPI 文档：`n` 个 operation，每个描述长 `pad`。
///
/// 用体积而不是 operation 数量去顶大小：200 个 operation 配 16 KB 描述既是 3.3 MB，
/// 解析侧的耗时却仍由 operation 数决定，故「>3 MB」这件事不会顺带把这条测试拖慢。
///
/// **单行压缩**：CDN 上的 spec 与 `JSON.stringify` 的产物都是这个形状，
/// 格式识别必须认得出它。
fn big_openapi(n: usize, pad: usize) -> String {
    let filler = "x".repeat(pad);
    let mut s = String::with_capacity(n * (pad + 400) + 1024);
    s.push_str(
        "{\"openapi\":\"3.0.3\",\"info\":{\"title\":\"大文档\",\"version\":\"1\"},\
         \"servers\":[{\"url\":\"https://api.example.com/v1\"}],\"paths\":{",
    );
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        // 路径名必须唯一，否则 JSON 里同一个键出现两次。
        // 收尾 4 个 `}`：200 → responses → get → 本条 path。paths 与根由下面那行收。
        s.push_str(&format!(
            "\"/p{i}\":{{\"get\":{{\"tags\":[\"t{}\"],\"summary\":\"s{i}\",\"description\":\"{filler}\",\"responses\":{{\"200\":{{\"description\":\"ok\"}}}}}}}}",
            i % 4
        ));
    }
    s.push_str("}}");
    s
}

/// 编排层把用户能读懂的理由装在 `Validation` 里，传输层失败也在那儿。
fn message_of(err: &AppError) -> String {
    match err {
        AppError::Validation(m) => m.clone(),
        other => panic!("应是一个用户能读懂的 validation 错, got: {other:?}"),
    }
}

fn tree_keys(node: &TransferNode, out: &mut Vec<String>) {
    out.extend(node.endpoints.iter().map(|e| e.key.clone()));
    for c in &node.children {
        tree_keys(c, out);
    }
}

fn all_tree_keys(nodes: &[TransferNode]) -> Vec<String> {
    let mut out = Vec::new();
    for n in nodes {
        tree_keys(n, &mut out);
    }
    out
}

#[tokio::test]
async fn a_multi_megabyte_document_survives_the_url_branch_intact() {
    let _policy = ScopedPolicy::with(false);
    let doc = big_openapi(200, 16_000);
    assert!(
        doc.len() > 3_000_000,
        "这份文档必须真的是 >3 MB, got {} 字节",
        doc.len()
    );
    let server = serve_doc(doc.clone()).await;
    let pool = temp_pool("large-url");
    let project_id = seed_project_with_module(&pool).0;

    let preview = import_preview(
        &pool,
        &ImportPreviewOpts {
            project_id,
            module_id: "m1".into(),
            content: None,
            url: Some(server.url("/openapi.yaml")),
        },
    )
    .await
    .expect(">3 MB 的合法文档必须能走完 URL 分支并解析出来");

    assert_eq!(
        preview.counts.endpoints, 200,
        "全部 operation 都要解析出来，一个都不能少"
    );
    assert_eq!(preview.format, "openapi-3");
    assert_eq!(
        preview.resolved_content.as_deref(),
        Some(doc.as_str()),
        "回传的原文必须与服务端拉到的是同一份字节"
    );
    assert_eq!(server.hits(), 1);
}

#[tokio::test]
async fn an_oversized_document_is_refused_by_a_message_naming_the_cap() {
    let _policy = ScopedPolicy::with(false);
    let doc = big_openapi(200, 44_000);
    assert!(
        doc.len() as u64 > FETCH_MAX_KB * 1024,
        "这份文档必须真的超过 {FETCH_MAX_KB} KB 上限, got {} 字节",
        doc.len()
    );
    let server = serve_doc(doc).await;
    let pool = temp_pool("oversize-url");
    let project_id = seed_project_with_module(&pool).0;

    let err = import_preview(
        &pool,
        &ImportPreviewOpts {
            project_id,
            module_id: "m1".into(),
            content: None,
            url: Some(server.url("/openapi.yaml")),
        },
    )
    .await
    .expect_err("超过上限的文档必须被拒");

    let msg = message_of(&err);
    assert!(
        msg.contains("上限") && msg.contains(&FETCH_MAX_KB.to_string()),
        "错误要指向那条大小上限并带上它的值, got: {msg}"
    );
    // 关键在「不是解析失败」：若不拦，用户看到的是一句「YAML 解析失败」，
    // 真正的原因（我们自己把它砍了）在任何地方都不会出现。
    assert!(
        !msg.contains("expected") && !msg.contains("解析失败"),
        "这不该是一条解析器的报错, got: {msg}"
    );
}

#[tokio::test]
async fn the_commit_never_refetches_what_the_preview_already_read() {
    let _policy = ScopedPolicy::with(false);
    let doc = big_openapi(12, 400);
    let server = serve_doc(doc.clone()).await;
    let pool = temp_pool("refetch-url");
    let (project_id, module_id) = seed_project_with_module(&pool);

    let preview = import_preview(
        &pool,
        &ImportPreviewOpts {
            project_id: project_id.clone(),
            module_id: module_id.clone(),
            content: None,
            url: Some(server.url("/openapi.yaml")),
        },
    )
    .await
    .expect("preview");
    assert_eq!(server.hits(), 1, "预览只该拉一次");

    // 渲染层把预览回传的原文原样送回 commit。
    let content = preview
        .resolved_content
        .clone()
        .expect("URL 模式必须回传原文，否则 commit 会再去拉一次");
    assert_eq!(content, doc, "送回去的必须就是刚拉到的那份字节");
    let selected = all_tree_keys(&preview.groups);
    assert_eq!(selected.len(), 12);

    let result = import_commit(
        &pool,
        &ImportCommitOpts {
            project_id,
            module_id,
            content,
            selected,
            apply_service: None,
        },
    )
    .expect("commit");

    assert_eq!(result.created_endpoints, 12);
    assert_eq!(
        server.hits(),
        1,
        "commit 必须用送回来的那份原文，不能再拉一次；再拉一次的后果是两次拉到的 \
         可能已经不是同一份文件了"
    );
}

#[tokio::test]
async fn a_refused_url_is_reported_without_ever_dialling() {
    let _policy = ScopedPolicy::with(true);
    let server = serve_doc(big_openapi(2, 100)).await;
    let pool = temp_pool("refused-url");
    let project_id = seed_project_with_module(&pool).0;

    let err = import_preview(
        &pool,
        &ImportPreviewOpts {
            project_id,
            module_id: "m1".into(),
            content: None,
            url: Some(server.url("/openapi.yaml")),
        },
    )
    .await
    .expect_err("严格策略下 loopback 必须被拒");

    let msg = message_of(&err);
    assert!(
        msg.contains(POLICY_ENV),
        "错误要说得出是哪条策略拒的，否则用户只会看到一个没有下一步的失败, got: {msg}"
    );
    assert_eq!(
        server.hits(),
        0,
        "被拒的 URL 一次都不该拨出去——`check_url` 在建客户端之前就短路了"
    );
}

#[tokio::test]
async fn the_declared_server_is_never_the_one_dialled() {
    // `servers[0]` 是文档**声明**的服务器，`url` 才是抓取目标。这两者混起来就会把
    // 「文档里写了个私网地址」误当成一次 SSRF 尝试——它不是，那只是一个要被记录成
    // 服务的字符串。默认策略下这一条真的能拉通，正说明声明的地址从未被访问。
    let _policy = ScopedPolicy::with(false);
    let doc = "{\n\"openapi\":\"3.0.3\",\n\"info\":{\"title\":\"云元数据\",\"version\":\"1\"},\n\
               \"servers\":[{\"url\":\"http://169.254.169.254/latest\"}],\n\
               \"paths\":{\"/ping\":{\"get\":{\"responses\":{\"200\":{\"description\":\"ok\"}}}}}}\n"
        .to_string();
    let server = serve_doc(doc).await;
    let pool = temp_pool("declared-server");
    let project_id = seed_project_with_module(&pool).0;

    let preview = import_preview(
        &pool,
        &ImportPreviewOpts {
            project_id,
            module_id: "m1".into(),
            content: None,
            url: Some(server.url("/openapi.json")),
        },
    )
    .await
    .expect("声明了一个私网 server 的文档照样应当拉得下来");

    assert_eq!(server.hits(), 1, "抓取目标只有那台 loopback");
    let svc = preview.service.expect("声明的 server 应当落成一个服务预告");
    assert_eq!(svc.service, "169.254.169.254");
}

// --- 测试脚手架 ---------------------------------------------------------

/// 一个项目 + 一个模块，别的什么都没有。`seed_one_endpoint` 那份预置了一个
/// `GET /users`，拿来跑大文档会把它标成重复。
fn seed_project_with_module(pool: &DbPool) -> (String, String) {
    db::with_tx(pool, |conn| {
        let project = db::projects::create_with_defaults(
            conn,
            &Project {
                id: "p1".into(),
                name: "示例".into(),
                description: None,
                base_url: None,
                base_url_definitions: Vec::new(),
                default_base_url_definition_id: None,
                color: "#3b82f6".into(),
                created_at: 0,
                updated_at: 0,
            },
        )?;
        db::nodes::create(
            conn,
            &ResourceNode {
                id: "m1".into(),
                root_id: project.id.clone(),
                parent_id: None,
                kind: ResourceNodeType::Module,
                name: "用户".into(),
                description: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            },
            None,
        )?;
        Ok((project.id, "m1".to_string()))
    })
    .expect("seed")
}

fn temp_pool(tag: &str) -> DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-api-{tag}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::init(Path::new(&dir)).expect("init")
}

fn endpoint_detail(node_id: &str, method: HttpMethod, url: &str) -> EndpointDetail {
    EndpointDetail {
        node_id: node_id.into(),
        method,
        url: url.into(),
        environment_id: None,
        base_url_definition_id: None,
        base_url_override: None,
        params: Vec::new(),
        headers: Vec::new(),
        body: Default::default(),
        auth: Default::default(),
        docs: None,
        notes: None,
        tags: Vec::new(),
    }
}

/// 建一个项目 + 模块 + 一个 `GET /users` 接口。
fn seed_one_endpoint(pool: &DbPool) -> String {
    db::with_tx(pool, |conn| {
        let project = db::projects::create_with_defaults(
            conn,
            &Project {
                id: "p1".into(),
                name: "示例".into(),
                description: None,
                base_url: None,
                base_url_definitions: Vec::new(),
                default_base_url_definition_id: None,
                color: "#3b82f6".into(),
                created_at: 0,
                updated_at: 0,
            },
        )?;
        let module = ResourceNode {
            id: "m1".into(),
            root_id: project.id.clone(),
            parent_id: None,
            kind: ResourceNodeType::Module,
            name: "用户".into(),
            description: None,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        db::nodes::create(conn, &module, None)?;
        db::nodes::create(
            conn,
            &ResourceNode {
                id: "e1".into(),
                root_id: project.id.clone(),
                parent_id: Some("m1".into()),
                kind: ResourceNodeType::Endpoint,
                name: "用户列表".into(),
                description: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            },
            Some(&endpoint_detail("e1", HttpMethod::Get, "/users")),
        )?;
        Ok(project.id)
    })
    .expect("seed")
}
