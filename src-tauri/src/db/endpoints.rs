//! CRUD for the `endpoints` table.
//!
//! Endpoints carry the bulk of the rich structure: params / headers
//! (KeyValue[]), body (RequestBody), auth (AuthConfig), and tags. All
//! four are stored as JSON-encoded TEXT columns; the (de)serialise
//! round-trip is centralised in [`row_to_endpoint`] and the
//! [`EndpointWrite`] extractor below.
//!
//! The renderer is the source of truth for ids; this layer accepts
//! whatever id the caller sends (validated for non-emptiness only).

use rusqlite::{params, Connection, Row};

use crate::domain::{Endpoint, EndpointPatch};
use crate::error::AppError;

// -------------------------------------------------------------------------
// Reads.
// -------------------------------------------------------------------------

/// All endpoints under a collection, ordered by `sort_order`.
pub fn list_by_collection(
    conn: &mut Connection,
    collection_id: &str,
) -> Result<Vec<Endpoint>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, collection_id, name, description, method, url, \
                environment_id, base_url_definition_id, base_url_override, \
                params_json, headers_json, body_json, auth_json, \
                docs, notes, tags_json, sort_order, created_at, updated_at \
         FROM endpoints \
         WHERE collection_id = ?1 \
         ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![collection_id], row_to_endpoint)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

pub fn get(conn: &mut Connection, id: &str) -> Result<Endpoint, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, collection_id, name, description, method, url, \
                environment_id, base_url_definition_id, base_url_override, \
                params_json, headers_json, body_json, auth_json, \
                docs, notes, tags_json, sort_order, created_at, updated_at \
         FROM endpoints WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => row_to_endpoint(row).map_err(AppError::Sqlite),
        None => Err(AppError::NotFound(format!("endpoint {id}"))),
    }
}

/// Multi-criteria search. All criteria are AND-combined; passing `None`
/// for a filter means "don't restrict on this dimension".
///
///   - `q`: case-insensitive substring match against `name`, `url`,
///     or `description`. Uses `LIKE '%q%'` so the caller should pass
///     the raw query (no wildcards required).
///   - `method`: exact match against the stored method string (caller
///     is responsible for upper-casing).
///   - `tags`: ANY match — endpoint's tags_json must contain the tag
///     as a JSON array element. Implemented via `LIKE '%"tag"%'` which
///     is good enough for the renderer's small tag space.
pub fn search(
    conn: &mut Connection,
    q: Option<&str>,
    method: Option<&str>,
    tags: Option<&[String]>,
) -> Result<Vec<Endpoint>, AppError> {
    let mut sql = String::from(
        "SELECT id, collection_id, name, description, method, url, \
               environment_id, base_url_definition_id, base_url_override, \
               params_json, headers_json, body_json, auth_json, \
               docs, notes, tags_json, sort_order, created_at, updated_at \
         FROM endpoints WHERE 1=1",
    );
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(q) = q {
        sql.push_str(" AND (name LIKE ? OR url LIKE ? OR description LIKE ?)");
        let pat = format!("%{}%", q);
        binds.push(rusqlite::types::Value::Text(pat.clone()));
        binds.push(rusqlite::types::Value::Text(pat.clone()));
        binds.push(rusqlite::types::Value::Text(pat));
    }
    if let Some(m) = method {
        sql.push_str(" AND method = ?");
        binds.push(rusqlite::types::Value::Text(m.to_string()));
    }
    if let Some(tags) = tags {
        for t in tags {
            // tags are JSON-encoded; look for the literal `"tag"` substring.
            sql.push_str(" AND tags_json LIKE ?");
            binds.push(rusqlite::types::Value::Text(format!("%\"{}\"%", t)));
        }
    }
    sql.push_str(" ORDER BY updated_at DESC, id ASC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(binds.iter()), row_to_endpoint)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

// -------------------------------------------------------------------------
// Writes.
// -------------------------------------------------------------------------

pub fn create(conn: &mut Connection, e: &Endpoint) -> Result<(), AppError> {
    let params_json = serde_json::to_string(&e.params)?;
    let headers_json = serde_json::to_string(&e.headers)?;
    let body_json = serde_json::to_string(&e.body)?;
    let auth_json = serde_json::to_string(&e.auth)?;
    let tags_json = serde_json::to_string(&e.tags)?;
    conn.execute(
        "INSERT INTO endpoints \
         (id, collection_id, name, description, method, url, \
          environment_id, base_url_definition_id, base_url_override, \
          params_json, headers_json, body_json, auth_json, \
          docs, notes, tags_json, sort_order, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        params![
            e.id,
            e.collection_id,
            e.name,
            e.description,
            e.method.as_str(),
            e.url,
            e.environment_id,
            e.base_url_definition_id,
            e.base_url_override,
            params_json,
            headers_json,
            body_json,
            auth_json,
            e.docs,
            e.notes,
            tags_json,
            e.sort_order,
            e.created_at,
            e.updated_at,
        ],
    )?;
    Ok(())
}

pub fn update(
    conn: &mut Connection,
    id: &str,
    patch: EndpointPatch,
) -> Result<Endpoint, AppError> {
    if patch.is_empty() {
        return get(conn, id);
    }
    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(name) = &patch.name {
        sets.push(format!("name = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(name.clone()));
    }
    if let Some(d) = &patch.description {
        sets.push(format!("description = ?{}", binds.len() + 1));
        binds.push(match d {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    if let Some(m) = &patch.method {
        sets.push(format!("method = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(m.as_str().to_string()));
    }
    if let Some(u) = &patch.url {
        sets.push(format!("url = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(u.clone()));
    }
    if let Some(e) = &patch.environment_id {
        sets.push(format!("environment_id = ?{}", binds.len() + 1));
        binds.push(match e {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    if let Some(b) = &patch.base_url_definition_id {
        sets.push(format!("base_url_definition_id = ?{}", binds.len() + 1));
        binds.push(match b {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    if let Some(b) = &patch.base_url_override {
        sets.push(format!("base_url_override = ?{}", binds.len() + 1));
        binds.push(match b {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    if let Some(p) = &patch.params {
        sets.push(format!("params_json = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(serde_json::to_string(p)?));
    }
    if let Some(h) = &patch.headers {
        sets.push(format!("headers_json = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(serde_json::to_string(h)?));
    }
    if let Some(b) = &patch.body {
        sets.push(format!("body_json = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(serde_json::to_string(b)?));
    }
    if let Some(a) = &patch.auth {
        sets.push(format!("auth_json = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(serde_json::to_string(a)?));
    }
    if let Some(d) = &patch.docs {
        sets.push(format!("docs = ?{}", binds.len() + 1));
        binds.push(match d {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    if let Some(n) = &patch.notes {
        sets.push(format!("notes = ?{}", binds.len() + 1));
        binds.push(match n {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    if let Some(t) = &patch.tags {
        sets.push(format!("tags_json = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(serde_json::to_string(t)?));
    }
    if let Some(s) = patch.sort_order {
        sets.push(format!("sort_order = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Integer(s));
    }
    // Always bump updated_at.
    sets.push(format!("updated_at = ?{}", binds.len() + 1));
    binds.push(rusqlite::types::Value::Integer(crate::domain::now_ms()));

    let sql = format!(
        "UPDATE endpoints SET {} WHERE id = ?{}",
        sets.join(", "),
        binds.len() + 1
    );
    binds.push(rusqlite::types::Value::Text(id.to_string()));
    let rows = conn.execute(&sql, rusqlite::params_from_iter(binds.iter()))?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("endpoint {id}")));
    }
    get(conn, id)
}

pub fn delete(conn: &mut Connection, id: &str) -> Result<(), AppError> {
    let rows = conn.execute("DELETE FROM endpoints WHERE id = ?1", params![id])?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("endpoint {id}")));
    }
    Ok(())
}

/// Duplicate an endpoint, returning the new id. All structured fields
/// (params, headers, body, auth, tags) round-trip through the JSON
/// columns; the copy lives in the same `collection_id` as the source
/// with a fresh `sort_order` placed AFTER the source's max sibling
/// (so the duplicate lands at the end of the collection — the
/// renderer reorders if it wants a different position).
///
/// MUST be called inside `db::with_tx`.
pub fn duplicate(conn: &mut Connection, id: &str) -> Result<String, AppError> {
    let mut ep = get(conn, id)?;

    // Fresh id, fresh timestamps, fresh sort_order at the tail.
    ep.id = crate::commands::uid();
    let now = crate::domain::now_ms();
    ep.created_at = now;
    ep.updated_at = now;

    // sort_order = MAX(sort_order in same collection) + 1. If the
    // collection is empty MAX is NULL → coalesce to -1 so +1 = 0.
    let max_sort: Option<i64> = conn
        .query_row(
            "SELECT MAX(sort_order) FROM endpoints WHERE collection_id = ?1",
            params![&ep.collection_id],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    ep.sort_order = max_sort.unwrap_or(-1) + 1;

    create(conn, &ep)?;
    Ok(ep.id)
}

/// Move one or many endpoints to a new sibling position in
/// `collection_id`. Repositions the endpoints and rewrites every
/// sibling's `sort_order` contiguously (0..N).
///
/// MUST be called inside `db::with_tx`.
///
/// Semantics match `collections::move_many`:
///   * `endpoint_ids` is the ordered list (caller order preserved)
///     of the items being moved.
///   * `target_index` is clamped to `[0, sibling_len]`.
///   * `collection_id` of the moved endpoints is updated to
///     `collection_id` (so this also handles cross-collection drags
///     — dropping a request into another folder rearranges the
///     target folder and reparents the request).
pub fn move_many(
    conn: &mut Connection,
    endpoint_ids: &[String],
    collection_id: &str,
    target_index: i32,
) -> Result<(), AppError> {
    // 1. Sibling list in display order.
    let mut stmt = conn.prepare(
        "SELECT id FROM endpoints \
         WHERE collection_id = ?1 \
         ORDER BY sort_order ASC, id ASC",
    )?;
    let mut current: Vec<String> = stmt
        .query_map(params![collection_id], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    // 2. Remove moving ids so they slot in at the new position.
    current.retain(|id| !endpoint_ids.iter().any(|m| m == id));

    // 3. Clamp target_index.
    let mut idx = if target_index < 0 { 0 } else { target_index as usize };
    if idx > current.len() {
        idx = current.len();
    }

    // 4. Insert in caller order starting at `idx`.
    for (offset, id) in endpoint_ids.iter().enumerate() {
        current.insert(idx + offset, id.clone());
    }

    // 5. Renumber sort_order and update collection_id for every row.
    //    updated_at is bumped so the touched rows reflect "I just got
    //    reordered" in the renderer's recently-changed views.
    let now = crate::domain::now_ms();
    let mut update = conn.prepare(
        "UPDATE endpoints SET sort_order = ?1, collection_id = ?2, updated_at = ?3 \
         WHERE id = ?4",
    )?;
    for (i, id) in current.iter().enumerate() {
        update.execute(params![i as i64, collection_id, now, id])?;
    }
    drop(update);

    Ok(())
}

// -------------------------------------------------------------------------
// Row mapper.
// -------------------------------------------------------------------------

fn row_to_endpoint(row: &Row<'_>) -> Result<Endpoint, rusqlite::Error> {
    let params_json: String = row.get(9)?;
    let headers_json: String = row.get(10)?;
    let body_json: String = row.get(11)?;
    let auth_json: String = row.get(12)?;
    let tags_json: String = row.get(15)?;
    let method_str: String = row.get(4)?;

    // JSON columns: serde failures get mapped to rusqlite's
    // `FromSqlConversionFailure` so the rest of the row still loads.
    // `rusqlite` wants `Box<dyn StdError + Send + Sync + 'static>`,
    // so we wrap the message in a tiny string-backed error type
    // defined at the bottom of this file.
    let parse_err = |e: serde_json::Error, col: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(JsonColError(format!("{col}: {e}"))),
        )
    };

    let params: Vec<crate::domain::KeyValue> = serde_json::from_str(&params_json)
        .map_err(|e| parse_err(e, "params_json"))?;
    let headers: Vec<crate::domain::KeyValue> = serde_json::from_str(&headers_json)
        .map_err(|e| parse_err(e, "headers_json"))?;
    let body: crate::domain::RequestBody = serde_json::from_str(&body_json)
        .map_err(|e| parse_err(e, "body_json"))?;
    let auth: crate::domain::AuthConfig = serde_json::from_str(&auth_json)
        .map_err(|e| parse_err(e, "auth_json"))?;
    let tags: Vec<String> = serde_json::from_str(&tags_json)
        .map_err(|e| parse_err(e, "tags_json"))?;

    let method = match method_str.as_str() {
        "GET" => crate::domain::HttpMethod::Get,
        "POST" => crate::domain::HttpMethod::Post,
        "PUT" => crate::domain::HttpMethod::Put,
        "PATCH" => crate::domain::HttpMethod::Patch,
        "DELETE" => crate::domain::HttpMethod::Delete,
        "HEAD" => crate::domain::HttpMethod::Head,
        "OPTIONS" => crate::domain::HttpMethod::Options,
        other => crate::domain::HttpMethod::Custom(other.to_string()),
    };

    Ok(Endpoint {
        id: row.get(0)?,
        collection_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        method,
        url: row.get(5)?,
        environment_id: row.get(6)?,
        base_url_definition_id: row.get(7)?,
        base_url_override: row.get(8)?,
        params,
        headers,
        body,
        auth,
        docs: row.get(13)?,
        notes: row.get(14)?,
        tags,
        sort_order: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}

/// Tiny `StdError` wrapper around a string. Needed because rusqlite
/// demands `Box<dyn StdError + Send + Sync + 'static>` for the
/// `FromSqlConversionFailure` payload, and a bare `String` doesn't
/// implement `StdError`.
#[derive(Debug)]
struct JsonColError(String);

impl std::fmt::Display for JsonColError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for JsonColError {}

// -------------------------------------------------------------------------
// Tests.
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::domain::{
        ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BodyMode, HttpMethod, KeyValue,
        RequestBody,
    };

    fn fresh_pool() -> db::DbPool {
        let dir = std::env::temp_dir().join(format!(
            "simple-post-endpoints-test-{}-{}",
            std::process::id(),
            crate::db::test_counter::next()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        db::init(&dir).expect("init")
    }

    fn sample_endpoint(id: &str) -> Endpoint {
        Endpoint {
            id: id.into(),
            collection_id: "c_login".into(),
            name: "Login".into(),
            description: None,
            method: HttpMethod::Post,
            url: "/login".into(),
            environment_id: None,
            base_url_definition_id: None,
            base_url_override: None,
            params: vec![],
            headers: vec![],
            body: RequestBody { mode: BodyMode::None, content: String::new(), schema: None },
            auth: AuthConfig { kind: AuthType::None, bearer: None, basic: None, apikey: None },
            docs: None,
            notes: None,
            tags: vec!["smoke".into()],
            sort_order: 0,
            created_at: 1,
            updated_at: 2,
        }
    }

    #[test]
    fn create_and_get_round_trip_preserves_json_columns() {
        let pool = fresh_pool();
        let mut ep = sample_endpoint("e_new");
        ep.params = vec![KeyValue {
            id: "p1".into(),
            key: "include".into(),
            value: "all".into(),
            enabled: true,
            description: None,
        }];
        ep.body = RequestBody {
            mode: BodyMode::Json,
            content: "{\"x\":1}".into(),
            schema: None,
        };
        ep.auth = AuthConfig {
            kind: AuthType::Apikey,
            bearer: None,
            basic: None,
            apikey: Some(ApiKeyAuth {
                key: "X-API-Key".into(),
                value: "abc".into(),
                in_: ApiKeyLocation::Header,
            }),
        };
        db::with_tx(&pool, |c| create(c, &ep)).unwrap();
        let got = db::with_conn(&pool, |c| get(c, "e_new")).unwrap();
        assert_eq!(got.params.len(), 1);
        assert_eq!(got.params[0].key, "include");
        assert_eq!(got.body.mode, BodyMode::Json);
        assert_eq!(got.auth.kind, AuthType::Apikey);
        assert_eq!(got.auth.apikey.unwrap().in_, ApiKeyLocation::Header);
    }

    #[test]
    fn search_by_query_substring() {
        let pool = fresh_pool();
        let ids: Vec<String> = db::with_conn(&pool, |c| {
            let rows = search(c, Some("短信"), None, None)?;
            let out: Vec<String> = rows.into_iter().map(|r| r.id).collect();
            Ok(out)
        })
        .unwrap();
        // The seeded endpoint name "短信登录" contains the substring
        assert!(ids.contains(&"e_login_sms".to_string()));
    }

    #[test]
    fn search_by_method_filters_correctly() {
        let pool = fresh_pool();
        let rows = db::with_conn(&pool, |c| search(c, None, Some("GET"), None)).unwrap();
        let methods: Vec<_> = rows.iter().map(|e| e.method.as_str().to_string()).collect();
        assert!(methods.iter().all(|m| m == "GET"));
        // Sanity: at least one GET endpoint seeded
        assert!(!rows.is_empty());
    }

    #[test]
    fn search_by_tag_uses_like_match() {
        let pool = fresh_pool();
        let tag = "登录".to_string();
        let rows =
            db::with_conn(&pool, |c| search(c, None, None, Some(&[tag.clone()]))).unwrap();
        let all_have_tag = rows
            .iter()
            .all(|e| e.tags.iter().any(|t| t == &tag));
        assert!(all_have_tag, "every result must carry the requested tag");
    }

    #[test]
    fn search_combines_criteria() {
        let pool = fresh_pool();
        let tag = "登录".to_string();
        let rows = db::with_conn(&pool, |c| {
            search(c, None, Some("POST"), Some(&[tag.clone()]))
        })
        .unwrap();
        assert!(rows
            .iter()
            .all(|e| e.method.as_str() == "POST" && e.tags.contains(&tag)));
    }

    #[test]
    fn update_changes_tags_json_column() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| {
            update(
                c,
                "e_login_sms",
                EndpointPatch {
                    tags: Some(vec!["公开".into(), "auth".into()]),
                    ..Default::default()
                },
            )?;
            Ok(())
        })
        .unwrap();
        let got = db::with_conn(&pool, |c| get(c, "e_login_sms")).unwrap();
        assert_eq!(got.tags, vec!["公开".to_string(), "auth".to_string()]);
    }

    #[test]
    fn update_can_clear_base_url_override() {
        let pool = fresh_pool();
        // First write a value
        db::with_tx(&pool, |c| {
            update(
                c,
                "e_login_sms",
                EndpointPatch {
                    base_url_override: Some(Some("http://override".into())),
                    ..Default::default()
                },
            )?;
            Ok(())
        })
        .unwrap();
        // Then clear it
        db::with_tx(&pool, |c| {
            update(
                c,
                "e_login_sms",
                serde_json::from_str(r#"{"baseUrlOverride": null}"#).unwrap(),
            )?;
            Ok(())
        })
        .unwrap();
        let got = db::with_conn(&pool, |c| get(c, "e_login_sms")).unwrap();
        assert!(got.base_url_override.is_none());
    }

    #[test]
    fn list_by_collection_returns_seeded_endpoints() {
        let pool = fresh_pool();
        let rows = db::with_conn(&pool, |c| list_by_collection(c, "c_login")).unwrap();
        let names: Vec<_> = rows.iter().map(|e| e.name.clone()).collect();
        assert!(names.iter().any(|n| n == "短信登录"));
        assert!(names.iter().any(|n| n == "账号密码登录"));
    }
}