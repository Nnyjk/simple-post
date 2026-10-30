//! 「这次发送按哪个环境解析」的归属判据。
//!
//! 环境里的变量属于它自己那个项目，故本次调用点名的环境必须属于端点所在的项目 ——
//! 拿别的项目的环境来替换，等于把那个项目的值读进这个项目的请求。
//! 端点行本身不记环境：用哪个环境是每次调用自己决定的。

use std::sync::atomic::{AtomicU32, Ordering};

/// 每次调用自增，用来在同毫秒内构造多个 state() 时区分临时目录
/// （cargo 默认并行跑测试）。
static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// 一个含两个项目与一个「别的项目的环境」的库。
fn state() -> std::sync::Arc<crate::mcp::AppState> {
    let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-env-resolve-{}-{}-{}",
        std::process::id(),
        simple_post_core::domain::now_ms(),
        id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::sync::Arc::new(crate::mcp::AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: std::time::Instant::now(),
        metrics: std::sync::Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// 本次调用点名的环境必须属于端点所在的项目。
#[test]
fn an_environment_from_another_project_is_not_resolved() {
    let st = state();
    let pool = &st.pool;

    simple_post_core::db::with_tx(pool, |c| {
        for (id, name) in [("p_own", "自己"), ("p_other", "别的")] {
            simple_post_core::db::projects::create(
                c,
                &simple_post_core::domain::Project {
                    id: id.to_string(),
                    name: name.to_string(),
                    description: None,
                    color: String::new(),
                    created_at: 0,
                    updated_at: 0,
                },
            )?;
        }
        let env = simple_post_core::domain::Environment {
            id: "env_foreign".to_string(),
            project_id: "p_other".to_string(),
            name: "dev".to_string(),
            module_base_urls: Default::default(),
            variables: [("token".to_string(), "secret".to_string())]
                .into_iter()
                .collect(),
            disabled_variables: Vec::new(),
            created_at: 0,
        };
        simple_post_core::db::environments::create(c, &env)?;
        Ok(())
    })
    .expect("种两个项目与一个别的项目的环境");

    let foreign = super::resolve_env(pool, Some("env_foreign"), "p_own").expect("解析环境");
    assert!(
        foreign.is_none(),
        "别的项目的环境不能被拿来替换变量：{foreign:?}"
    );

    let own = super::resolve_env(pool, Some("env_foreign"), "p_other").expect("解析环境");
    assert!(own.is_some(), "同一个项目的环境照旧能解析出来");
}
