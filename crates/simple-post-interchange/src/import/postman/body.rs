//! `body.mode` 的降级映射：表达不了的形态出诊断，能表达的落到 `RequestBody`。

use serde_json::{Map, Value};
use simple_post_core::domain::{BodyMode, RequestBody};

use crate::diagnostics::Diagnostic;
use crate::spec::MultipartPart;

use super::{field_text, kv_list, Ctx};

pub(super) fn parse_body(req: &Map<String, Value>, ptr: &str, ctx: &mut Ctx) -> RequestBody {
    let Some(body) = req.get("body") else {
        return RequestBody::default();
    };
    let Some(b) = body.as_object() else {
        ctx.diagnostics
            .push(Diagnostic::warn(ptr, "body 不是 JSON 对象，按无 body 处理").dropping("body"));
        return RequestBody::default();
    };

    match field_text(b, "mode").unwrap_or_default().as_str() {
        "raw" => {
            let raw = field_text(b, "raw").unwrap_or_default();
            // Postman 的 raw 也可能本来就是 JSON 文本；language 缺失时按文本存。
            let language = b
                .get("options")
                .and_then(|o| o.get("raw"))
                .and_then(|r| r.get("language"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let mode = if language.eq_ignore_ascii_case("json") {
                BodyMode::Json
            } else {
                BodyMode::Raw
            };
            RequestBody {
                mode,
                content: raw,
                schema: None,
            }
        }
        "urlencoded" => {
            // 指针指到 `urlencoded` 那一层：body 的下一层就是它。指 body 本身的话，
            // 用户看到的 `/item/0/request/body/2` 在源文件里指向一个**数组**，而那一项
            // 其实住在 `body/urlencoded/2` —— 对不上就是定位不到。
            let parts = kv_list(b.get("urlencoded"), &format!("{ptr}/urlencoded"), ctx);
            // 不做百分号编码：值里常有 `{{var}}`，编码后模板化就匹配不到了。
            let content = parts
                .iter()
                .filter(|p| p.enabled)
                .map(|p| format!("{}={}", p.key, p.value))
                .collect::<Vec<_>>()
                .join("&");
            RequestBody {
                mode: BodyMode::Form,
                content,
                schema: None,
            }
        }
        "formdata" => {
            let mut parts: Vec<MultipartPart> = Vec::new();
            if let Some(arr) = b.get("formdata").and_then(Value::as_array) {
                for (i, entry) in arr.iter().enumerate() {
                    let Some(e) = entry.as_object() else {
                        ctx.diagnostics.push(Diagnostic::warn(
                            format!("{ptr}/formdata/{i}"),
                            "这一项不是 JSON 对象，已跳过",
                        ));
                        continue;
                    };
                    let name = field_text(e, "key").unwrap_or_default();
                    if field_text(e, "type").as_deref().unwrap_or("text") == "file" {
                        ctx.diagnostics.push(
                            Diagnostic::warn(
                                format!("{ptr}/formdata/{i}"),
                                "multipart 的 file 字段无法导入（本仓库只存文本分片）",
                            )
                            .dropping(format!("multipart file 字段 {name}")),
                        );
                        continue;
                    }
                    // 分片结构没有 enabled 位，故被 Postman 禁用的文本字段照样落下。
                    parts.push(MultipartPart {
                        name,
                        value: field_text(e, "value").unwrap_or_default(),
                        filename: None,
                        content_type: None,
                    });
                }
            }
            RequestBody {
                mode: BodyMode::Multipart,
                content: serde_json::to_string(&parts).unwrap_or_default(),
                schema: None,
            }
        }
        "graphql" => {
            ctx.diagnostics.push(
                Diagnostic::warn(ptr, "graphql 请求体没有对应的标准形态，已丢弃")
                    .dropping("graphql 请求体"),
            );
            RequestBody::default()
        }
        "file" => {
            ctx.diagnostics
                .push(Diagnostic::warn(ptr, "二进制文件请求体无法导入").dropping("file 请求体"));
            RequestBody::default()
        }
        "" => RequestBody::default(),
        other => {
            ctx.diagnostics
                .push(Diagnostic::warn(ptr, "未知的 body mode，已按无 body 处理").dropping(other));
            RequestBody::default()
        }
    }
}
