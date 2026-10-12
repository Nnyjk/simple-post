use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::db;
use crate::error::AppError;

use super::super::{map_app_error, HttpErrorResponse, WebState};

use super::super::ensure_server_id;

pub(crate) async fn projects_list(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = db::with_conn(&s.pool, db::projects::list).map_err(map_app_error)?;
    Ok(Json(
        serde_json::to_value(rows).unwrap_or_else(|_| json!([])),
    ))
}

pub(crate) async fn projects_get(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_conn(&s.pool, |c| db::projects::get(c, &id)).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(row).unwrap_or(Value::Null)))
}

/// Create a project, seeding its default environment (and, when a base
/// URL was supplied, a named service holding it).
///
/// Two body shapes land here, and both are live:
///
/// - `{ "name": …, "opts": { description, color, baseUrl } }` — what
///   `invokeCmd(COMMANDS.projectsCreate, …)` sends, because
///   `WEB_ROUTES.projectsCreate` has no `body:` mapper and the args
///   object *is* the payload. This is the SPA's own contract.
/// - a flat `Project` — the documented public v1 shape, which an
///   external REST client uses.
///
/// They used to disagree, and the disagreement was silent: the route
/// parsed a `Project`, `Project` has no `opts` field, and serde discards
/// unknown keys without complaint. So in `serve-web` the create form's
/// description, colour **and** base URL were all discarded and the
/// project came back with only its name — which looked exactly like the
/// form not working rather than like a transport mismatch.
///
/// `opts` wins when present; otherwise the flat fields are read off the
/// body. A base URL is not among them: it is not a project attribute,
/// and `Project.base_url` is legacy and never persisted.
pub(crate) async fn projects_create(
    State(s): State<Arc<WebState>>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    let opts: crate::commands::dto::CreateProjectOpts = match body.get("opts") {
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?,
        None => crate::commands::dto::CreateProjectOpts {
            description: body
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
            color: body
                .get("color")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
    };
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| map_app_error(AppError::Validation("missing field 'name'".to_string())))?;

    let now = crate::domain::now_ms();
    let mut project: crate::domain::Project = crate::domain::Project {
        id: String::new(),
        name,
        description: opts.description,
        base_url: None,
        base_url_definitions: Vec::new(),
        default_base_url_definition_id: None,
        color: opts
            .color
            .unwrap_or_else(crate::domain::default_project_color),
        created_at: now,
        updated_at: now,
    };
    ensure_server_id(&mut body);
    if let Some(id) = body.get("id").and_then(Value::as_str) {
        if !id.is_empty() {
            project.id = id.to_string();
        } else {
            project.id = crate::domain::uid();
        }
    } else {
        project.id = crate::domain::uid();
    }

    let created = db::with_tx(&s.pool, |c| db::projects::create_with_defaults(c, &project))
        .map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&created).unwrap_or(Value::Null)),
    ))
}

pub(crate) async fn projects_update(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::ProjectPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // `db::with_tx` takes an `Fn` closure (not `FnOnce`), so we can't
    // move `patch` in — clone it. The struct is a small flat object.
    let row = db::with_tx(&s.pool, |c| db::projects::update(c, &id, patch.clone()))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(row).unwrap_or(Value::Null)))
}

pub(crate) async fn projects_delete(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::projects::delete(c, &id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}
