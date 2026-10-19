//! `parameters` 的两级合并，与 `requestBody.content` 按偏好序择一。

use std::collections::HashMap;

use serde_json::{Map, Value};
use simple_post_core::domain::{BodyMode, KeyValue, RequestBody};

use crate::diagnostics::Diagnostic;
use crate::spec::escape_pointer_token;

use super::refs::deref;
use super::{normalize_media, scalar_text, text, Ctx, DocView, CONTENT_PREFERENCE};

/// path-item 级 + operation 级的 `parameters`，后者覆盖前者。返回 (query, header)。
///
/// `in: path` 的参数已经在路径模板里，`in: cookie` 在 `ApiKeyLocation` 里没有对应变体，
/// 两者都出诊断后跳过。
pub(super) fn parameters(
    ctx: &mut Ctx,
    view: &DocView,
    item: &Map<String, Value>,
    operation: &Map<String, Value>,
    pointer: &str,
) -> (Vec<KeyValue>, Vec<KeyValue>) {
    let merged = merged_parameters(ctx, view, item, operation, pointer);
    let mut params = Vec::new();
    let mut headers = Vec::new();
    for (i, value) in merged.iter().enumerate() {
        let at = format!("{pointer}/parameters/{i}");
        let Some(p) = value.as_object() else {
            ctx.diagnostics
                .push(Diagnostic::warn(&at, "这一项不是对象，已跳过").dropping(value.to_string()));
            continue;
        };
        let Some(name) = text(p, "name") else {
            ctx.diagnostics
                .push(Diagnostic::warn(&at, "参数没有 name，已跳过").dropping(value.to_string()));
            continue;
        };
        let at = format!("{at}/{}", escape_pointer_token(&name));
        // 只有显式 `required: false` 才禁用；3.1 的联合类型数组等价于可选。
        let enabled = !matches!(p.get("required"), Some(Value::Bool(false)));
        let default = p
            .get("schema")
            .and_then(|s| s.get("default"))
            .map(scalar_text)
            .unwrap_or_default();
        let description = text(p, "description");

        match text(p, "in").unwrap_or_default().as_str() {
            "query" => params.push(KeyValue {
                key: name,
                value: default,
                enabled,
                description,
                ..Default::default()
            }),
            "header" => headers.push(KeyValue {
                key: name,
                value: default,
                enabled,
                description,
                ..Default::default()
            }),
            "path" => ctx.diagnostics.push(Diagnostic::info(
                &at,
                "path 参数已经在路径模板里，不需要单独导入",
            )),
            "cookie" => ctx.diagnostics.push(
                Diagnostic::warn(
                    &at,
                    "ApiKeyLocation 只有 header 与 query，cookie 参数无法导入",
                )
                .dropping(format!("cookie {name}")),
            ),
            other => ctx.diagnostics.push(
                Diagnostic::warn(&at, format!("未知的参数位置 {other:?}，已跳过"))
                    .dropping(format!("{other} {name}")),
            ),
        }
    }
    (params, headers)
}

/// 两级 `parameters` 合并成一份，同名同 `in` 算同一个参数。
fn merged_parameters(
    ctx: &mut Ctx,
    view: &DocView,
    item: &Map<String, Value>,
    operation: &Map<String, Value>,
    pointer: &str,
) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for source in [item, operation] {
        let Some(raw) = source.get("parameters") else {
            continue;
        };
        let at_ptr = format!("{pointer}/parameters");
        let Some(Value::Array(entries)) = deref(ctx, view.root, raw, &at_ptr) else {
            continue;
        };
        for entry in entries {
            let key = param_identity(&entry);
            match at.get(&key) {
                Some(&slot) => out[slot] = entry,
                None => {
                    at.insert(key, out.len());
                    out.push(entry);
                }
            }
        }
    }
    out
}

/// 一个参数的身份。认不出身份（不是对象 / 没有 name）时退回成它自己的文本，
/// 否则这些条目会互相覆盖成一个。
fn param_identity(entry: &Value) -> String {
    match (entry.get("name"), entry.get("in")) {
        (Some(Value::String(name)), Some(Value::String(loc))) => format!("{loc}:{name}"),
        _ => format!("#{}", entry),
    }
}

/// `requestBody.content` 按偏好序取一个，其余 media type 逐个点名丢弃。
pub(super) fn request_body(
    ctx: &mut Ctx,
    view: &DocView,
    operation: &Map<String, Value>,
    pointer: &str,
) -> RequestBody {
    let Some(raw) = operation.get("requestBody") else {
        // 没有 requestBody 就是没有请求体，不是「待补」。
        return RequestBody::default();
    };
    let at_ptr = format!("{pointer}/requestBody");
    let Some(body) = deref(ctx, view.root, raw, &at_ptr) else {
        return RequestBody::default();
    };
    let Some(Value::Object(content)) = body.get("content") else {
        ctx.diagnostics.push(Diagnostic::warn(
            &at_ptr,
            "requestBody 没有 content，已按无请求体导入",
        ));
        return RequestBody::default();
    };

    let picked = CONTENT_PREFERENCE
        .iter()
        .find_map(|want| {
            content
                .keys()
                .find(|key| normalize_media(key) == *want)
                .cloned()
        })
        // 偏好序都没命中就取第一个，形态存进 Raw 而不是假装知道它是什么。
        .or_else(|| content.keys().next().cloned());

    let Some(picked) = picked else {
        ctx.diagnostics.push(Diagnostic::warn(
            format!("{at_ptr}/content"),
            "requestBody.content 是空的，已按无请求体导入",
        ));
        return RequestBody::default();
    };
    for key in content.keys() {
        if key != &picked {
            ctx.diagnostics.push(
                Diagnostic::warn(
                    format!("{at_ptr}/content/{}", escape_pointer_token(key)),
                    "一个请求体只装得下一种 media type，其余的无法导入",
                )
                .dropping(key.clone()),
            );
        }
    }

    let sample = content[&picked]
        .get("example")
        .or_else(|| content[&picked].get("default"));
    RequestBody {
        mode: mode_of(&normalize_media(&picked)),
        content: sample.map(scalar_text).unwrap_or_default(),
        schema: None,
    }
}

/// media type → [`BodyMode`]。`application/json` 与 `*+json` 都算 JSON。
fn mode_of(media: &str) -> BodyMode {
    match media {
        "application/x-www-form-urlencoded" => BodyMode::Form,
        "multipart/form-data" => BodyMode::Multipart,
        m if m == "application/json" || m.ends_with("+json") => BodyMode::Json,
        _ => BodyMode::Raw,
    }
}
