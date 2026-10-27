//! 引用关系：哪些端点引用了哪些变量名。

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::error::AppError;

/// 一条动态变量被哪些端点引用。
#[derive(Debug, Clone)]
pub struct Reference {
    pub endpoint_id: String,
    pub endpoint_name: String,
    pub project_id: String,
    pub project_name: String,
}

/// 哪些端点引用了哪些变量名。跨全部项目——定义是全局的，引用它的端点可能散在基线项目
/// 与各个分发项目里。
///
/// 一次遍历回答所有名字：逐条定义各查一次会在 N 条定义时扫 N 遍全表，故这里把
/// 「哪个端点引用了哪些名字」一次算出来。返回的 map 里每个被问的名字都有一项（可能
/// 是空列表）。
///
/// 粗筛是「这一行里有没有 `{{`」——多数端点不引用任何变量，故不必解析它的 JSON；细查用
/// `crate::template::list_missing_vars` 逐个字符串确认，因为值里可能恰好含 `{{` 却没有
/// 真正的标记。
pub fn referencing_endpoints(
    conn: &mut Connection,
    names: &[String],
) -> Result<HashMap<String, Vec<Reference>>, AppError> {
    let mut out: HashMap<String, Vec<Reference>> = HashMap::new();
    if names.is_empty() {
        return Ok(out);
    }
    let wanted: HashSet<&str> = names.iter().map(String::as_str).collect();
    // 每个被问的名字都先进 map（列表为空）：调用方按名字取即可，不必区分
    // 「没有引用」与「这个名字根本没被问过」。
    for name in names {
        out.entry(name.clone()).or_default();
    }

    let mut stmt = conn.prepare(
        "SELECT d.node_id, n.name, n.root_id, COALESCE(p.name, ''), \
                d.url, d.params_json, d.headers_json, d.body_json, d.auth_json \
         FROM endpoint_details d \
         JOIN nodes n ON n.id = d.node_id \
         LEFT JOIN projects p ON p.id = n.root_id \
         ORDER BY n.root_id, n.sort_order, n.id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
        ))
    })?;

    for r in rows {
        let (
            node_id,
            node_name,
            project_id,
            project_name,
            url,
            params_json,
            headers_json,
            body_json,
            auth_json,
        ) = r.map_err(AppError::Sqlite)?;
        if !url.contains("{{")
            && !params_json.contains("{{")
            && !headers_json.contains("{{")
            && !body_json.contains("{{")
            && !auth_json.contains("{{")
        {
            continue;
        }
        let mut texts: Vec<String> = vec![url];
        for json in [params_json, headers_json, body_json, auth_json] {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
                collect_markers_from_json(&value, &mut texts);
            }
        }
        let mut hits: Vec<String> = Vec::new();
        for text in &texts {
            for name in crate::template::list_missing_vars(text) {
                if wanted.contains(name.as_str()) && !hits.contains(&name) {
                    hits.push(name);
                }
            }
        }
        for name in hits {
            out.entry(name).or_default().push(Reference {
                endpoint_id: node_id.clone(),
                endpoint_name: node_name.clone(),
                project_id: project_id.clone(),
                project_name: project_name.clone(),
            });
        }
    }
    Ok(out)
}

/// 把一个 JSON 值里出现的所有字符串收集出来（键与值都要——值里的 `{{name}}` 也算数）。
fn collect_markers_from_json(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_markers_from_json(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                out.push(k.clone());
                collect_markers_from_json(v, out);
            }
        }
        _ => {}
    }
}
