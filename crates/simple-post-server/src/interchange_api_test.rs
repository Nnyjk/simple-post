//! 编排层：内容来源、IR → 线形状的投影、文件名、往返。

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{
    EndpointDetail, HttpMethod, Project, ResourceNode, ResourceNodeType,
};
use simple_post_core::error::AppError;
use simple_post_interchange::fingerprint::FingerprintSet;
use simple_post_interchange::ir::{Exchange, IrEndpoint, IrModule};
use simple_post_interchange::Diagnostic;

use super::{
    all_endpoints, count_groups, diagnostics_of, file_name, mark_duplicates, project_modules,
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

// --- 测试脚手架 ---------------------------------------------------------

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
