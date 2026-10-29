use super::*;

#[test]
fn at_least_self_always_holds() {
    assert!(permission_at_least(Permission::Read, Permission::Read));
    assert!(permission_at_least(
        Permission::ReadCall,
        Permission::ReadCall
    ));
    assert!(permission_at_least(Permission::Full, Permission::Full));
}

#[test]
fn at_least_higher_covers_lower() {
    assert!(permission_at_least(Permission::ReadCall, Permission::Read));
    assert!(permission_at_least(Permission::Full, Permission::Read));
    assert!(permission_at_least(Permission::Full, Permission::ReadCall));
}

#[test]
fn at_least_lower_does_not_cover_higher() {
    assert!(!permission_at_least(Permission::Read, Permission::ReadCall));
    assert!(!permission_at_least(Permission::Read, Permission::Full));
    assert!(!permission_at_least(Permission::ReadCall, Permission::Full));
}

#[test]
fn required_permission_read_class() {
    assert_eq!(required_permission("env_list"), Permission::Read);
    assert_eq!(required_permission("env_get_active"), Permission::Read);
    assert_eq!(required_permission("project_list"), Permission::Read);
    assert_eq!(required_permission("project_get"), Permission::Read);
    assert_eq!(required_permission("module_list"), Permission::Read);
    assert_eq!(required_permission("collection_list"), Permission::Read);
    assert_eq!(required_permission("endpoint_list"), Permission::Read);
    assert_eq!(required_permission("endpoint_get"), Permission::Read);
    assert_eq!(required_permission("endpoint_search"), Permission::Read);
    assert_eq!(required_permission("base_url_def_list"), Permission::Read);
    assert_eq!(
        required_permission("api_resolve_template"),
        Permission::Read
    );
    assert_eq!(
        required_permission("api_resolve_endpoint"),
        Permission::Read
    );
    assert_eq!(
        required_permission("api_validate_endpoint"),
        Permission::Read
    );
    assert_eq!(required_permission("api_parse_response"), Permission::Read);
    assert_eq!(required_permission("api_diff_responses"), Permission::Read);
    assert_eq!(
        required_permission("api_extract_jsonpath"),
        Permission::Read
    );
}

#[test]
fn required_permission_read_call_class() {
    assert_eq!(required_permission("api_run_request"), Permission::ReadCall);
    assert_eq!(
        required_permission("api_call_endpoint"),
        Permission::ReadCall
    );
}

#[test]
fn required_permission_defaults_unknown_to_full() {
    // Write tools — keep them out of the match so they fall here.
    assert_eq!(required_permission("projects_create"), Permission::Full);
    assert_eq!(required_permission("projects_update"), Permission::Full);
    assert_eq!(required_permission("projects_delete"), Permission::Full);
    assert_eq!(required_permission("endpoints_create"), Permission::Full);
    // Genuinely unknown name also defaults to Full (safe side).
    assert_eq!(required_permission("not_a_real_tool"), Permission::Full);
}

#[test]
fn permission_string_roundtrip() {
    for p in [Permission::Read, Permission::ReadCall, Permission::Full]
        .iter()
        .copied()
    {
        assert_eq!(Permission::parse(p.as_str()), Some(p));
    }
}

#[test]
fn permission_string_rejects_garbage() {
    // `parse` is the domain-layer helper; my code only depends on
    // `Permission::as_str` being round-trippable, so the same cases
    // exercise the wrong-direction failure we care about.
    assert_eq!(Permission::parse("Read"), None); // wrong case
    assert_eq!(Permission::parse("admin"), None);
    assert_eq!(Permission::parse(""), None);
    assert_eq!(Permission::parse("full_control"), None);
}

#[test]
fn ranks_form_total_order() {
    // Sanity: ranks must be strictly increasing.
    assert!(rank(Permission::Read) < rank(Permission::ReadCall));
    assert!(rank(Permission::ReadCall) < rank(Permission::Full));
}

// ============================================================
// CapabilityPolicy — global MCP gate
// ============================================================

#[test]
fn capability_policy_default_is_read_only() {
    // Safe-by-default: an unclassified or unknown tool must never be
    // treated as permitted.
    assert_eq!(CapabilityPolicy::default(), CapabilityPolicy::ReadOnly);
}

#[test]
fn capability_policy_round_trip() {
    for p in [
        CapabilityPolicy::ReadOnly,
        CapabilityPolicy::SafeWrite,
        CapabilityPolicy::FullAccess,
    ] {
        assert_eq!(CapabilityPolicy::parse(p.as_str()), Some(p));
        assert_eq!(p.as_str(), p.as_str());
    }
    assert_eq!(CapabilityPolicy::parse("nope"), None);
    assert_eq!(CapabilityPolicy::parse(""), None);
    // Wrong case — wire format is snake_case and case-sensitive.
    assert_eq!(CapabilityPolicy::parse("Read_Only"), None);
    assert_eq!(CapabilityPolicy::parse("READ_ONLY"), None);
}

#[test]
fn capability_policy_satisfies_matrix() {
    // ReadOnly — covers Read only.
    assert!(CapabilityPolicy::ReadOnly.satisfies(Capability::Read));
    assert!(!CapabilityPolicy::ReadOnly.satisfies(Capability::Create));
    assert!(!CapabilityPolicy::ReadOnly.satisfies(Capability::Update));
    assert!(!CapabilityPolicy::ReadOnly.satisfies(Capability::Delete));
    assert!(!CapabilityPolicy::ReadOnly.satisfies(Capability::Send));

    // SafeWrite — covers Read/Create/Update but NOT Delete/Send.
    assert!(CapabilityPolicy::SafeWrite.satisfies(Capability::Read));
    assert!(CapabilityPolicy::SafeWrite.satisfies(Capability::Create));
    assert!(CapabilityPolicy::SafeWrite.satisfies(Capability::Update));
    assert!(!CapabilityPolicy::SafeWrite.satisfies(Capability::Delete));
    assert!(!CapabilityPolicy::SafeWrite.satisfies(Capability::Send));

    // FullAccess — covers everything.
    assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Read));
    assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Create));
    assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Update));
    assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Delete));
    assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Send));
}

#[test]
fn capability_policy_serializes_to_snake_case_wire() {
    // Wire form the renderer reads back must stay snake_case even
    // though Rust's default serializer would emit "ReadOnly".
    assert_eq!(
        serde_json::to_string(&CapabilityPolicy::ReadOnly).unwrap(),
        "\"read_only\""
    );
    assert_eq!(
        serde_json::to_string(&CapabilityPolicy::SafeWrite).unwrap(),
        "\"safe_write\""
    );
    assert_eq!(
        serde_json::to_string(&CapabilityPolicy::FullAccess).unwrap(),
        "\"full_access\""
    );
}

#[test]
fn capability_policy_deserializes_from_snake_case_wire() {
    let ro: CapabilityPolicy = serde_json::from_str("\"read_only\"").unwrap();
    assert_eq!(ro, CapabilityPolicy::ReadOnly);
    let sw: CapabilityPolicy = serde_json::from_str("\"safe_write\"").unwrap();
    assert_eq!(sw, CapabilityPolicy::SafeWrite);
    let fa: CapabilityPolicy = serde_json::from_str("\"full_access\"").unwrap();
    assert_eq!(fa, CapabilityPolicy::FullAccess);
    // Wrong-case must NOT round-trip — locks down the wire form.
    assert!(serde_json::from_str::<CapabilityPolicy>("\"Read_Only\"").is_err());
}

// ============================================================
// resolve_capability — global × per-project conservative merge
// ============================================================

/// 建一个位于临时目录的新连接池。镜像 `db::preferences::tests` 里的辅助函数。
fn fresh_pool() -> crate::db::DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-cap-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    crate::db::init(&dir).expect("init")
}

#[tokio::test]
async fn resolve_capability_global_default_is_read_only() {
    let pool = fresh_pool();
    let cap = resolve_capability(&pool, None).await.unwrap();
    assert_eq!(cap, CapabilityPolicy::ReadOnly);
}

#[tokio::test]
async fn resolve_capability_global_preference_takes_when_no_project() {
    let pool = fresh_pool();
    crate::db::with_tx(&pool, |c| {
        crate::db::preferences::set(
            c,
            crate::db::preferences::keys::MCP_POLICY,
            &"full_access".to_string(),
        )
    })
    .unwrap();
    let cap = resolve_capability(&pool, None).await.unwrap();
    assert_eq!(cap, CapabilityPolicy::FullAccess);
}

#[tokio::test]
async fn resolve_capability_takes_stricter_side_when_project_permits_more() {
    // global = ReadOnly, project = Full ⇒ effective: ReadOnly
    // (user's choice to globally block wins over per-project grant)
    let pool = fresh_pool();
    // FK requires the project to exist before permissions.set.
    let now = crate::domain::now_ms();
    crate::db::with_tx(&pool, |c| {
        crate::db::projects::create(
            c,
            &crate::domain::Project {
                id: "p_full".to_string(),
                name: "p_full".to_string(),
                description: None,
                base_url: None,
                base_url_definitions: vec![],
                default_base_url_definition_id: None,
                color: crate::domain::default_project_color(),
                created_at: now,
                updated_at: now,
            },
        )
    })
    .unwrap();
    crate::db::with_tx(&pool, |c| {
        crate::db::preferences::set(
            c,
            crate::db::preferences::keys::MCP_POLICY,
            &"read_only".to_string(),
        )
    })
    .unwrap();
    crate::db::with_tx(&pool, |c| {
        crate::db::permissions::set(c, "p_full", Permission::Full)
    })
    .unwrap();
    let cap = resolve_capability(&pool, Some("p_full")).await.unwrap();
    assert_eq!(cap, CapabilityPolicy::ReadOnly);
}

#[tokio::test]
async fn resolve_capability_takes_stricter_side_when_global_permits_more() {
    // global = FullAccess, project = ReadCall ⇒ effective: SafeWrite
    // (per-project restrictive grant wins even when global is open)
    let pool = fresh_pool();
    // FK requires the project to exist before permissions.set.
    let now = crate::domain::now_ms();
    crate::db::with_tx(&pool, |c| {
        crate::db::projects::create(
            c,
            &crate::domain::Project {
                id: "p_readcall".to_string(),
                name: "p_readcall".to_string(),
                description: None,
                base_url: None,
                base_url_definitions: vec![],
                default_base_url_definition_id: None,
                color: crate::domain::default_project_color(),
                created_at: now,
                updated_at: now,
            },
        )
    })
    .unwrap();
    crate::db::with_tx(&pool, |c| {
        crate::db::preferences::set(
            c,
            crate::db::preferences::keys::MCP_POLICY,
            &"full_access".to_string(),
        )
    })
    .unwrap();
    crate::db::with_tx(&pool, |c| {
        crate::db::permissions::set(c, "p_readcall", Permission::ReadCall)
    })
    .unwrap();
    let cap = resolve_capability(&pool, Some("p_readcall")).await.unwrap();
    assert_eq!(cap, CapabilityPolicy::SafeWrite);
}

#[tokio::test]
async fn resolve_capability_agree() {
    // both sides agree ⇒ that tier
    let pool = fresh_pool();
    // FK requires the project to exist before permissions.set.
    let now = crate::domain::now_ms();
    crate::db::with_tx(&pool, |c| {
        crate::db::projects::create(
            c,
            &crate::domain::Project {
                id: "p_match".to_string(),
                name: "p_match".to_string(),
                description: None,
                base_url: None,
                base_url_definitions: vec![],
                default_base_url_definition_id: None,
                color: crate::domain::default_project_color(),
                created_at: now,
                updated_at: now,
            },
        )
    })
    .unwrap();
    crate::db::with_tx(&pool, |c| {
        crate::db::preferences::set(
            c,
            crate::db::preferences::keys::MCP_POLICY,
            &"safe_write".to_string(),
        )
    })
    .unwrap();
    crate::db::with_tx(&pool, |c| {
        crate::db::permissions::set(c, "p_match", Permission::ReadCall)
    })
    .unwrap();
    let cap = resolve_capability(&pool, Some("p_match")).await.unwrap();
    assert_eq!(cap, CapabilityPolicy::SafeWrite);
}

#[tokio::test]
async fn resolve_capability_missing_project_id_falls_back_to_global() {
    // resolve_capability(Some("nonexistent")) — the per-project
    // lookup reads `default()` (ReadCall) when no row exists. So
    // global=FullAccess + missing project ⇒ effective: SafeWrite.
    let pool = fresh_pool();
    crate::db::with_tx(&pool, |c| {
        crate::db::preferences::set(
            c,
            crate::db::preferences::keys::MCP_POLICY,
            &"full_access".to_string(),
        )
    })
    .unwrap();
    let cap = resolve_capability(&pool, Some("nonexistent"))
        .await
        .unwrap();
    assert_eq!(cap, CapabilityPolicy::SafeWrite);
}
