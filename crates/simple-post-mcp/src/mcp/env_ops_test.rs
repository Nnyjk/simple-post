//! `env_update` 的入参形状：补丁类型自己报错，handler 不做逐字段 `.get()`。
//!
//! 两条回归网：拼错的键必须被拒绝且库里那行不变；`env_get` 读出来的整行改一处后
//! 传回必须成功，未点名的列保持原值。

use super::*;

use simple_post_core::db::{environments, permissions, projects};
use simple_post_core::domain::{Environment, ModuleBaseUrl, Permission, Project};

/// 每次调用自增，用来在同毫秒内构造多个 state() 时区分临时目录
/// （cargo 默认并行跑测试）。
static STATE_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn state() -> std::sync::Arc<AppState> {
    let id = STATE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-env-update-{}-{}-{}",
        std::process::id(),
        simple_post_core::domain::now_ms(),
        id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::sync::Arc::new(AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: std::time::Instant::now(),
    })
}

/// 一个项目 + 一个带变量与模块地址的环境，权限给到 `Full`（`env_update` 要这一档）。
fn seed(st: &AppState) {
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(
            c,
            &Project {
                id: "p_env".into(),
                name: "P".into(),
                description: None,
                color: "#303030".into(),
                created_at: 0,
                updated_at: 0,
            },
        )?;
        environments::create(
            c,
            &Environment {
                id: "env_patch".into(),
                project_id: "p_env".into(),
                name: "dev".into(),
                module_base_urls: [(
                    "m_env".to_string(),
                    ModuleBaseUrl {
                        url: "http://example.test".into(),
                        rewrites: Vec::new(),
                    },
                )]
                .into_iter()
                .collect(),
                variables: [("token".to_string(), "abc".to_string())]
                    .into_iter()
                    .collect(),
                disabled_variables: vec![],
                created_at: 0,
            },
        )?;
        permissions::set(c, "p_env", Permission::Full)
    })
    .unwrap();
}

#[tokio::test]
async fn env_update_rejects_a_misspelled_key_and_leaves_the_row_alone() {
    let st = state();
    seed(&st);

    let res = tool_env_update(
        &st,
        json!({ "env_id": "env_patch", "patch": { "nmae": "renamed" } }),
    )
    .await;

    match &res {
        Err(AppError::Validation(msg)) => assert!(
            msg.contains("nmae"),
            "错误文案必须点出写错的键，实际是：{msg}"
        ),
        other => panic!("拼错的键必须被拒绝，而不是静默成功：{other:?}"),
    }

    let stored =
        simple_post_core::db::with_conn(&st.pool, |c| environments::get(c, "env_patch")).unwrap();
    assert_eq!(stored.name, "dev");
    assert_eq!(
        stored.variables.get("token").map(String::as_str),
        Some("abc")
    );
}

#[tokio::test]
async fn env_update_accepts_a_row_that_was_read_back() {
    let st = state();
    seed(&st);

    let before =
        simple_post_core::db::with_conn(&st.pool, |c| environments::get(c, "env_patch")).unwrap();
    let mut row = serde_json::to_value(&before).unwrap();
    row["name"] = json!("renamed");

    tool_env_update(&st, json!({ "env_id": "env_patch", "patch": row }))
        .await
        .expect("整行回传必须成功");

    let after =
        simple_post_core::db::with_conn(&st.pool, |c| environments::get(c, "env_patch")).unwrap();
    assert_eq!(after.name, "renamed");
    assert_eq!(after.id, before.id);
    assert_eq!(after.project_id, before.project_id);
    assert_eq!(
        after.variables, before.variables,
        "未点名的字段必须保持原值"
    );
    assert_eq!(
        serde_json::to_value(&after.module_base_urls).unwrap(),
        serde_json::to_value(&before.module_base_urls).unwrap(),
        "未点名的字段必须保持原值"
    );
    assert_eq!(after.disabled_variables, before.disabled_variables);
}

/// 一格里可以放多条改写规则，顺序就是优先级，落库后原样读回。
#[tokio::test]
async fn env_set_baseurl_stores_the_rewrites_in_order() {
    let st = state();
    seed(&st);

    tool_env_set_baseurl(
        &st,
        json!({
            "env_id": "env_patch",
            "module_id": "m_env",
            "url": "http://example.test",
            "rewrites": [
                { "from": "/api/air", "to": "/v1/air" },
                { "from": "/api", "to": "/v2" },
            ],
        }),
    )
    .await
    .expect("合法的多条规则应当写入");

    let stored =
        simple_post_core::db::with_conn(&st.pool, |c| environments::get(c, "env_patch")).unwrap();
    let rewrites: Vec<(&str, &str)> = stored.module_base_urls["m_env"]
        .rewrites
        .iter()
        .map(|r| (r.from.as_str(), r.to.as_str()))
        .collect();
    assert_eq!(
        rewrites,
        vec![("/api/air", "/v1/air"), ("/api", "/v2")],
        "顺序本身是契约"
    );
}

/// 同一格里重复的起点（末尾斜杠的差异也算）被写入口拒绝，且那一格保持原样。
#[tokio::test]
async fn env_set_baseurl_rejects_duplicate_starts() {
    let st = state();
    seed(&st);

    let res = tool_env_set_baseurl(
        &st,
        json!({
            "env_id": "env_patch",
            "module_id": "m_env",
            "url": "http://example.test",
            "rewrites": [
                { "from": "/api/air", "to": "/v1" },
                { "from": "/api/air/", "to": "/v2" },
            ],
        }),
    )
    .await;

    match res {
        Err(simple_post_core::error::AppError::Validation(_)) => {}
        other => panic!("重复起点必须报校验错误，得到 {other:?}"),
    }
    let stored =
        simple_post_core::db::with_conn(&st.pool, |c| environments::get(c, "env_patch")).unwrap();
    assert!(
        stored.module_base_urls["m_env"].rewrites.is_empty(),
        "被拒绝的写入不得留下半份规则"
    );
}
