//! `http_request` 的参数面必须与 REST body 逐字段对应。
//!
//! `#[tauri::command]` **按参数名**去 payload 里取值（它把参数名转成 lowerCamelCase
//! 当作 payload 的键），而 REST 侧 `Json<HttpSendBody>` 直接吃整个 body。渲染层两边
//! 发的是同一个扁平对象，于是参数名必须就是那些 camelCase 字段名本身。
//!
//! 对不上时的症状是**只坏桌面、不坏浏览器**：桌面侧报
//! `invalid args … missing required key …`，浏览器侧一切正常。那正是 dual-transport
//! invariant 要消灭的那类半失效，故用源码形状把它钉住——与
//! `commands/interchange_test.rs` 同一手法。

/// 按顶层逗号切分——泛型与元组里的逗号不是分隔符。
fn split_top_level(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, ch) in text.char_indices() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// 取一段声明里的名字（`pub foo: Bar` → `foo`）。
///
/// 一段里可能先有属性行（`#[serde(default)]`），故取**最后**一行非属性的声明。
fn declared_name(part: &str) -> Option<String> {
    let mut name: Option<String> = None;
    for line in part.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        let field = line.split(':').next()?.trim();
        let field = field.strip_prefix("pub ").unwrap_or(field).trim();
        if field.is_empty() {
            return None;
        }
        name = Some(field.to_string());
    }
    name
}

/// Tauri 命令的参数名（Rust 蛇形）。
fn tauri_params(source: &str) -> Vec<String> {
    let start = source
        .find("async fn http_request(")
        .expect("http_request 命令必须存在");
    let body = &source[start..];
    let open = body.find('(').expect("签名必须有括号");
    let end = body[open..].find(") ->").expect("命令签名必须闭合") + open;
    split_top_level(&body[open + 1..end])
        .iter()
        .filter_map(|part| declared_name(part))
        .filter(|name| name != "state")
        .collect()
}

/// REST body 的字段名（Rust 蛇形）。
fn body_fields(source: &str) -> Vec<String> {
    let start = source
        .find("pub struct HttpSendBody")
        .expect("HttpSendBody 必须存在");
    let body = &source[start..];
    let open = body.find('{').expect("结构体必须有花括号");
    let end = body[open..].find("\n}").expect("结构体必须闭合") + open;
    split_top_level(&body[open + 1..end])
        .iter()
        .filter_map(|part| declared_name(part))
        .collect()
}

/// 蛇形 → lowerCamelCase，与 Tauri 的参数名推导规则一致。
fn camel(name: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for ch in name.chars() {
        if ch == '_' {
            upper = true;
            continue;
        }
        if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[test]
fn the_command_parameters_match_the_rest_body_fields() {
    let command = tauri_params(include_str!("lib.rs"));
    let fields = body_fields(include_str!(
        "../../crates/simple-post-server/src/web_api/dto.rs"
    ));

    let command_names: Vec<String> = command.iter().map(|n| camel(n)).collect();
    let field_names: Vec<String> = fields.iter().map(|n| camel(n)).collect();

    assert_eq!(
        command_names, field_names,
        "Tauri 命令的参数必须与 REST body 的字段一一对应；\
         对不上时桌面侧会报 missing required key，而浏览器侧照常工作"
    );
}

#[test]
fn the_script_context_is_part_of_the_send_payload() {
    let fields = body_fields(include_str!(
        "../../crates/simple-post-server/src/web_api/dto.rs"
    ));
    for wanted in ["endpoint_id", "env"] {
        assert!(
            fields.iter().any(|f| f == wanted),
            "body 必须带 {wanted}：没有它，脚本链装配不出来"
        );
    }
}
