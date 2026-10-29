//! `parameters` 的两级合并，与 `requestBody.content` 按偏好序择一。

use std::collections::HashMap;

use serde_json::{Map, Value};
use simple_post_core::domain::{BodyMode, KeyValue, RequestBody};

use crate::diagnostics::Diagnostic;
use crate::spec::{escape_pointer_token, MultipartPart};

use super::refs::deref;
use super::{normalize_media, scalar_text, text, Ctx, DocView, CONTENT_PREFERENCE};

/// path-item 级 + operation 级的 `parameters`，后者覆盖前者。
/// 返回 (query, path, header)。
///
/// `in: path` 的参数变成端点自己的**路径参数**：它们替换进路径本身
/// （`/users/{id}` 的 `id`），不像 query 参数那样进查询串。`in: cookie` 在
/// `ApiKeyLocation` 里没有对应变体，出诊断后跳过。
pub(super) fn parameters(
    ctx: &mut Ctx,
    view: &DocView,
    item: &Map<String, Value>,
    operation: &Map<String, Value>,
    pointer: &str,
) -> (Vec<KeyValue>, Vec<KeyValue>, Vec<KeyValue>) {
    let merged = merged_parameters(ctx, view, item, operation, pointer);
    let mut params = Vec::new();
    let mut path_params = Vec::new();
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
            "path" => {
                if default.is_empty() {
                    // 文档没给默认值时，路径参数的值是空的：导入后要自己填。
                    // 不说一声，用户看到的是一条发不出去的请求。
                    ctx.diagnostics.push(Diagnostic::info(
                        &at,
                        format!("路径参数 {name} 没有默认值，导入后需要自己填值"),
                    ));
                }
                path_params.push(KeyValue {
                    key: name,
                    value: default,
                    enabled,
                    description,
                    ..Default::default()
                })
            }
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
    (params, path_params, headers)
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

    let picked_media = &content[&picked];
    let media = normalize_media(&picked);
    let mode = mode_of(&media);
    if mode == BodyMode::Multipart {
        return multipart_body(ctx, view, picked_media, &at_ptr);
    }
    let sample = picked_media
        .get("example")
        .or_else(|| picked_media.get("default"));
    RequestBody {
        mode,
        content: sample.map(scalar_text).unwrap_or_default(),
        schema: import_schema(ctx, view, picked_media, &at_ptr),
    }
}

/// `content[media].schema` → `body.schema`（一份 JSON Schema 文本）。
///
/// **本地 `$ref` 展开成内联内容。** `body.schema` 存的是一段**自足**的文本：导出器
/// 把它写进 `components.schemas` 并让 `requestBody` 指过去，而那份 components 是导出
/// 时自己生成的——存一段带 `$ref` 的文本进去，导出时那份引用指不到任何东西。
///
/// 只收**对象**：布尔 schema（`true` / `false`）不是导出器认的那种（它要写成
/// `components.schemas` 里的一个对象），收进来只会在导出时变成一条「不是合法的
/// JSON Schema 对象」的诊断。故这里就不收，也说一句。
fn import_schema(ctx: &mut Ctx, view: &DocView, media: &Value, at_ptr: &str) -> Option<String> {
    let raw = media.get("schema")?;
    // deref 遇到成环或指不到东西时返回 None，并且已经为它出过诊断。
    let expanded = deref(ctx, view.root, raw, at_ptr)?;
    if !expanded.is_object() {
        ctx.diagnostics.push(Diagnostic::info(
            at_ptr,
            "请求体上的 schema 不是一个 JSON Schema 对象，这份定义没有导入",
        ));
        return None;
    }
    Some(expanded.to_string())
}

/// multipart 请求体 → 分片数组。
///
/// `BodyMode::Multipart` 的 `content` 装的是**分片数组的 JSON 文本**，不是样例原文：
/// 装错的话发送路径解析不出数组，得到零分片，样例静默消失。
///
/// 分片从两处来，先到先得：
///   * media type 上的 `example` / `default` 对象——它就是这份 body 的样例，键是字段名；
///   * `schema.properties`——没有样例时按字段名合成，值取该 property 的
///     `example` → `default` → 标量文本。
///
/// `filename` 留空：JSON schema 表达不了文件名，本仓库只存文本分片。
///
/// 两处都取不到分片时降级成 `BodyMode::Raw` 并出诊断：保守，但**不静默**——
/// 声称自己能发 multipart 却发不出分片，就是「存得对、发得错」。
///
/// **降级时那份 schema 不跟着走**：Raw 的 media type 是 `application/octet-stream`，
/// 而 multipart 的 schema 说的是有哪些表单字段。把它挂在 octet-stream 上，导出的文档
/// 就在宣称一个错的请求格式——而存下来的内容已经是原始文本了，这份定义描述的也不是它。
fn multipart_body(ctx: &mut Ctx, view: &DocView, media: &Value, at_ptr: &str) -> RequestBody {
    let mut parts = parts_from_example(media);
    if parts.is_empty() {
        parts = parts_from_properties(media);
    }
    // multipart 的 schema 也是一份正经的请求体定义（它说的是有哪些表单字段），
    // 故照样导入——不导入的话，「导出再导入」会把这份定义丢掉。
    let schema = import_schema(ctx, view, media, at_ptr);
    if parts.is_empty() {
        ctx.diagnostics.push(
            Diagnostic::warn(
                at_ptr,
                "multipart 请求体既没有样例、schema 里也没有字段，已按原始文本导入",
            )
            .dropping("multipart 分片与请求体定义"),
        );
        return RequestBody {
            mode: BodyMode::Raw,
            content: media
                .get("example")
                .or_else(|| media.get("default"))
                .map(scalar_text)
                .unwrap_or_default(),
            // **降级成 Raw 之后，这份定义不能跟着走。** Raw 的 media type 是
            // `application/octet-stream`，而 multipart 的 schema 说的是有哪些表单
            // 字段；把它挂在 octet-stream 上，导出的文档就在宣称一个错的请求格式。
            // 存下来的内容已经是原始文本了，这份定义描述的也不是它。
            schema: None,
        };
    }
    RequestBody {
        mode: BodyMode::Multipart,
        content: serde_json::to_string(&parts).unwrap_or_default(),
        schema,
    }
}

/// media type 上的样例对象 → 分片。键是表单字段名。
fn parts_from_example(media: &Value) -> Vec<MultipartPart> {
    let Some(sample) = media.get("example").or_else(|| media.get("default")) else {
        return Vec::new();
    };
    let Some(map) = sample.as_object() else {
        return Vec::new();
    };
    map.iter()
        .map(|(name, value)| MultipartPart {
            name: name.clone(),
            value: scalar_text(value),
            filename: None,
            content_type: None,
        })
        .collect()
}

/// `schema.properties` → 分片。值取该 property 的 `example` → `default` → 标量文本。
fn parts_from_properties(media: &Value) -> Vec<MultipartPart> {
    let Some(Value::Object(properties)) = media.get("schema").and_then(|s| s.get("properties"))
    else {
        return Vec::new();
    };
    properties
        .iter()
        .map(|(name, prop)| MultipartPart {
            name: name.clone(),
            value: prop
                .get("example")
                .or_else(|| prop.get("default"))
                .map(scalar_text)
                .unwrap_or_default(),
            filename: None,
            content_type: None,
        })
        .collect()
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
