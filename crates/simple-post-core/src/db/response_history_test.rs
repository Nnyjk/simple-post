use super::*;
use crate::db;
use crate::db::nodes;
use crate::domain::{
    AuthConfig, EndpointDetail, HttpMethod, Project, RequestBody, ResourceNode, ResourceNodeType,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

// fixture

fn node(id: &str, kind: ResourceNodeType, root: &str, parent: Option<&str>) -> ResourceNode {
    ResourceNode {
        id: id.into(),
        root_id: root.into(),
        parent_id: parent.map(str::to_string),
        kind,
        name: id.into(),
        description: None,
        sort_order: 0,
        created_at: 1,
        updated_at: 1,
    }
}

fn project(id: &str) -> Project {
    Project {
        id: id.into(),
        name: id.into(),
        description: None,
        default_base_url_definition_id: None,
        base_url_definitions: vec![],
        base_url: None,
        color: "#000000".into(),
        created_at: 1,
        updated_at: 1,
    }
}

/// `nodes::create` 拒绝没有 detail 行的 endpoint 节点，故 fixture 得为每个 endpoint 造一个。
/// 这里没有人读它。
fn detail(node_id: &str) -> EndpointDetail {
    EndpointDetail {
        node_id: node_id.into(),
        method: HttpMethod::Get,
        url: "/api/things".into(),
        environment_id: None,
        base_url_definition_id: None,
        base_url_override: None,
        params: vec![],
        path_params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
    }
}

/// 两个项目，各带一个模块与两个 endpoint——足以把「每 endpoint」与「每项目」区分开，
/// 也足以证明级联一路走到项目。
fn fresh_pool() -> (db::DbPool, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-response-history-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = db::init(&dir).expect("init");
    db::with_tx(&pool, |c| {
        for (pid, mid) in [("p1", "m1"), ("p2", "m2")] {
            db::projects::create(c, &project(pid))?;
            nodes::create(c, &node(mid, ResourceNodeType::Module, pid, None), None)?;
        }
        for id in ["e1", "e2"] {
            nodes::create(
                c,
                &node(id, ResourceNodeType::Endpoint, "p1", Some("m1")),
                Some(&detail(id)),
            )?;
        }
        nodes::create(
            c,
            &node("e3", ResourceNodeType::Endpoint, "p2", Some("m2")),
            Some(&detail("e3")),
        )?;
        Ok(())
    })
    .expect("seed tree");
    (pool, dir)
}

fn resp(status: u16, body: &str) -> StoredResponse {
    StoredResponse {
        status,
        status_text: "OK".into(),
        duration_ms: 12,
        size_bytes: body.len() as u64,
        headers: BTreeMap::from([("content-type".to_string(), "application/json".to_string())]),
        body: body.into(),
        body_json: Some(serde_json::json!({ "echo": body })),
        content_type: "application/json".into(),
        error: None,
        method: Some(crate::domain::HttpMethod::Get),
        url: Some("https://example.test/api".into()),
    }
}

fn count_for(pool: &db::DbPool, endpoint_id: &str) -> i64 {
    db::with_conn(pool, |c| {
        c.query_row(
            "SELECT COUNT(*) FROM response_history WHERE endpoint_id = ?1",
            params![endpoint_id],
            |r| r.get(0),
        )
        .map_err(AppError::Sqlite)
    })
    .unwrap()
}

// 往返

#[test]
fn every_field_survives_the_round_trip() {
    let (pool, _dir) = fresh_pool();
    let stored = db::with_conn(&pool, |c| list_by_endpoint(c, "e1")).unwrap();
    assert!(stored.is_empty());

    let sent = StoredResponse {
        error: Some("upstream said no".into()),
        url: None,
        method: Some(crate::domain::HttpMethod::Delete),
        body_json: None,
        ..resp(204, "")
    };
    db::with_tx(&pool, |c| append(c, "e1", &sent)).unwrap();

    let back = db::with_conn(&pool, |c| list_by_endpoint(c, "e1")).unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(
        back[0].response, sent,
        "stored response must equal what went in"
    );
    assert_eq!(back[0].endpoint_id, "e1");
    assert!(!back[0].body_truncated);
    // 缺席的可选字段保持缺席而非变成空字符串——渲染层正是用 `=== undefined` 分支来判断
    // 一行到底有没有请求上下文。
    assert_eq!(back[0].response.url, None);
    assert_eq!(back[0].response.body_json, None);
    assert_eq!(back[0].response.error.as_deref(), Some("upstream said no"));
}

#[test]
fn header_order_is_stable_across_reads() {
    let (pool, _dir) = fresh_pool();
    let mut headers = BTreeMap::new();
    for name in ["x-b", "x-a", "x-c"] {
        headers.insert(name.to_string(), "1".to_string());
    }
    let sent = StoredResponse {
        headers,
        ..resp(200, "ok")
    };
    db::with_tx(&pool, |c| append(c, "e1", &sent)).unwrap();

    let first = db::with_conn(&pool, |c| list_by_endpoint(c, "e1")).unwrap();
    let second = db::with_conn(&pool, |c| list_by_endpoint(c, "e1")).unwrap();
    // 进去时是 BTreeMap，出来时是 SQLite 文本：要点是同一行读两次一致，好让重渲染头部的
    // UI 不会在两次发送之间重排。
    let keys: Vec<&String> = first[0].response.headers.keys().collect();
    assert_eq!(keys, vec!["x-a", "x-b", "x-c"]);
    assert_eq!(first, second);
}

// 每 endpoint 的封顶

#[test]
fn append_keeps_only_the_last_cap_per_endpoint() {
    let (pool, _dir) = fresh_pool();
    // 刻意超额追加：那个会无上限撑大库的循环，正是必须不生效的循环。
    for i in 0..(RESPONSE_HISTORY_PER_ENDPOINT * 4) {
        db::with_tx(&pool, |c| append(c, "e1", &resp(200, &format!("body-{i}")))).unwrap();
    }
    assert_eq!(count_for(&pool, "e1"), RESPONSE_HISTORY_PER_ENDPOINT);

    let rows = db::with_conn(&pool, |c| list_by_endpoint(c, "e1")).unwrap();
    assert_eq!(rows.len(), RESPONSE_HISTORY_PER_ENDPOINT as usize);
    // 最新的在前，且存活的是**最后**那几次发送——一个丢掉最新行的封顶看起来是对的，
    // 直到用户去找他刚触发的那个响应。
    assert_eq!(
        rows[0].response.body,
        format!("body-{}", RESPONSE_HISTORY_PER_ENDPOINT * 4 - 1)
    );
    assert_eq!(
        rows[rows.len() - 1].response.body,
        format!("body-{}", RESPONSE_HISTORY_PER_ENDPOINT * 3)
    );
}

#[test]
fn the_cap_is_per_endpoint_not_global() {
    let (pool, _dir) = fresh_pool();
    for i in 0..(RESPONSE_HISTORY_PER_ENDPOINT + 5) {
        db::with_tx(&pool, |c| append(c, "e1", &resp(200, &format!("a{i}")))).unwrap();
    }
    db::with_tx(&pool, |c| append(c, "e2", &resp(201, "solo"))).unwrap();

    assert_eq!(count_for(&pool, "e1"), RESPONSE_HISTORY_PER_ENDPOINT);
    assert_eq!(
        count_for(&pool, "e2"),
        1,
        "e2 must not inherit e1's trimming"
    );
}

// 响应体封顶

#[test]
fn an_oversized_body_is_clipped_and_says_so() {
    let (pool, _dir) = fresh_pool();
    let huge = "x".repeat(RESPONSE_HISTORY_MAX_BODY_BYTES + 5_000);
    let entry = db::with_tx(&pool, |c| append(c, "e1", &resp(200, &huge))).unwrap();

    assert!(entry.body_truncated);
    assert!(entry.response.body.len() <= RESPONSE_HISTORY_MAX_BODY_BYTES);

    let back = db::with_conn(&pool, |c| list_by_endpoint(c, "e1")).unwrap();
    assert!(back[0].body_truncated, "the flag has to survive the write");
    assert!(
        back[0].response.body_json.is_none(),
        "a pretty-printed copy of the FULL body next to a clipped `body` \
         would show the viewer two different answers to the same question"
    );
    assert_eq!(back[0].response.body.len(), entry.response.body.len());
}

#[test]
fn a_body_at_the_cap_is_not_truncated() {
    let (pool, _dir) = fresh_pool();
    let exact = "x".repeat(RESPONSE_HISTORY_MAX_BODY_BYTES);
    let entry = db::with_tx(&pool, |c| append(c, "e1", &resp(200, &exact))).unwrap();
    assert!(!entry.body_truncated);
    assert_eq!(entry.response.body.len(), exact.len());
    assert!(entry.response.body_json.is_some());
}

#[test]
fn clipping_lands_on_a_char_boundary() {
    // 3 字节的中日韩字符，故朴素的字节切片会把其中一个切掉一半，阅读器会在标记本该出现的
    // 位置渲染出 U+FFFD。
    let body = "中".repeat(RESPONSE_HISTORY_MAX_BODY_BYTES / 2);
    let (clipped, truncated) = clip_body(&body);
    assert!(truncated);
    assert!(clipped.len() <= RESPONSE_HISTORY_MAX_BODY_BYTES);
    assert!(
        clipped.chars().all(|c| c == '中'),
        "clipping split a multi-byte character"
    );
    assert!(
        body.chars().collect::<String>().starts_with(&clipped),
        "the clipped prefix must be a prefix of the original"
    );
}

// 级联清理

#[test]
fn deleting_an_endpoint_takes_its_history() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| append(c, "e1", &resp(200, "a"))).unwrap();
    db::with_tx(&pool, |c| append(c, "e2", &resp(200, "b"))).unwrap();

    db::with_tx(&pool, |c| nodes::delete(c, "e1")).unwrap();

    assert_eq!(count_for(&pool, "e1"), 0);
    assert_eq!(
        count_for(&pool, "e2"),
        1,
        "one endpoint's delete must not reach a sibling's history"
    );
}

#[test]
fn deleting_a_project_takes_the_whole_subtree_s_history() {
    let (pool, _dir) = fresh_pool();
    for id in ["e1", "e2", "e3"] {
        db::with_tx(&pool, |c| append(c, id, &resp(200, id))).unwrap();
    }
    // 没有显式的历史清理——外键就是全部机制，故这个测试是将来某次重构与「留下一堆没人能
    // 触及或删除的孤儿行」之间唯一的挡板。
    db::with_tx(&pool, |c| db::projects::delete(c, "p1")).unwrap();

    assert_eq!(count_for(&pool, "e1"), 0);
    assert_eq!(count_for(&pool, "e2"), 0);
    assert_eq!(
        count_for(&pool, "e3"),
        1,
        "p2's endpoint is not p1's subtree"
    );
}

#[test]
fn clear_drops_one_endpoints_rows_and_reports_the_count() {
    let (pool, _dir) = fresh_pool();
    for i in 0..3 {
        db::with_tx(&pool, |c| append(c, "e1", &resp(200, &format!("{i}")))).unwrap();
    }
    let n = db::with_tx(&pool, |c| clear(c, "e1")).unwrap();
    assert_eq!(n, 3);
    assert_eq!(count_for(&pool, "e1"), 0);
    assert_eq!(
        db::with_tx(&pool, |c| clear(c, "e1")).unwrap(),
        0,
        "clearing an already-empty endpoint is 0, not an error"
    );
}

// 每 endpoint 的读取

#[test]
fn list_by_endpoint_returns_that_endpoint_and_nothing_else() {
    let (pool, _dir) = fresh_pool();
    for (id, tag) in [("e1", "one"), ("e2", "two"), ("e3", "three")] {
        db::with_tx(&pool, |c| append(c, id, &resp(200, tag))).unwrap();
    }
    db::with_tx(&pool, |c| append(c, "e1", &resp(500, "boom"))).unwrap();

    let rows = db::with_conn(&pool, |c| list_by_endpoint(c, "e1")).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter().all(|r| r.endpoint_id == "e1"),
        "a sibling endpoint's rows must never appear in this list — the \
         renderer keys the map by `endpointId` without re-checking"
    );
    // Newest first, which is what lets the strip drop its own reverse.
    assert_eq!(rows[0].response.body, "boom");
    assert_eq!(rows[1].response.body, "one");
}

#[test]
fn an_endpoint_with_no_history_reads_as_empty_not_as_everybody_elses() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| append(c, "e1", &resp(200, "one"))).unwrap();

    // The read this function replaced was project-scoped and returned
    // every endpoint's rows. Asking an empty endpoint for its history is
    // the single most common request the strip makes — every endpoint
    // the user opens for the first time — so "empty" has to be a real
    // answer rather than a fallback to something bigger.
    assert!(db::with_conn(&pool, |c| list_by_endpoint(c, "e2"))
        .unwrap()
        .is_empty());
    assert!(db::with_conn(&pool, |c| list_by_endpoint(c, "nope"))
        .unwrap()
        .is_empty());
}

// -------------------------------------------------------------------------
// The draft-endpoint contract
// -------------------------------------------------------------------------

#[test]
fn an_endpoint_that_is_not_a_row_cannot_be_stored() {
    let (pool, _dir) = fresh_pool();
    // A renderer-only draft endpoint — `draft:<uid>` per
    // `DRAFT_ID_PREFIX` in `src/stores/store-types.ts`. It has no
    // `nodes` row, so the foreign key is what stops the write. This
    // test pins that the database rejects it, because the renderer
    // side of the rule ("skip persistence for drafts") is a silent
    // branch: if it ever regresses, this is the error the user would
    // otherwise only see as a vanished history row.
    let err = db::with_tx(&pool, |c| append(c, "draft:abc123", &resp(200, "hi")));
    assert!(
        err.is_err(),
        "FK must reject an endpoint id with no nodes row"
    );
}
