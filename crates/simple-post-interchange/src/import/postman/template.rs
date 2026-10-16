//! 扫落库字段里的 `{{...}}`：替换链不认的 token 出声，一份文件里同一个只报一次。

use simple_post_core::domain::AuthType;
use simple_post_core::template::is_word;

use crate::diagnostics::Diagnostic;
use crate::ir::IrEndpoint;

use super::Ctx;

/// 扫一个 endpoint 所有落库字段里的 `{{...}}`。
///
/// 只认 `\w+`：其余两种形状都会被发送链路原样放过，不出声就是「导入成功、发送时不替换」。
pub(super) fn scan_templates(ep: &IrEndpoint, ptr: &str, ctx: &mut Ctx) {
    let url_ptr = format!("{ptr}/request/url");
    let header_ptr = format!("{ptr}/request/header");
    let body_ptr = format!("{ptr}/request/body");
    let auth_ptr = format!("{ptr}/request/auth");

    scan_template(&ep.path, &url_ptr, ctx);
    for p in &ep.params {
        scan_template(&p.value, &url_ptr, ctx);
    }
    for h in &ep.headers {
        scan_template(&h.value, &header_ptr, ctx);
    }
    scan_template(&ep.body.content, &body_ptr, ctx);

    // `AuthConfig` 是「判别字段 + 可选载荷」的扁平结构，未选中方案的载荷同样为真，
    // 故只读 `kind` 选中的那一份。
    match ep.auth.kind {
        AuthType::Basic => {
            if let Some(b) = &ep.auth.basic {
                scan_template(&b.username, &auth_ptr, ctx);
                scan_template(&b.password, &auth_ptr, ctx);
            }
        }
        AuthType::Bearer => {
            if let Some(t) = &ep.auth.bearer {
                scan_template(t, &auth_ptr, ctx);
            }
        }
        AuthType::Apikey => {
            if let Some(a) = &ep.auth.apikey {
                scan_template(&a.key, &auth_ptr, ctx);
                scan_template(&a.value, &auth_ptr, ctx);
            }
        }
        AuthType::None | AuthType::Oauth2 => {}
    }
}

/// 一个字段里的 `{{...}}`。同一个坏 token 在一份文件里只报一次。
///
/// 标记名的判据走 [`is_word`]——与发送链路同一份，所以诊断说的话和实际行为不会分家。
fn scan_template(text: &str, ptr: &str, ctx: &mut Ctx) {
    if text.is_empty() {
        return;
    }
    // 扫的是 ASCII 的 `{}`；UTF-8 续字节都 ≥ 0x80，按字节推进不会切在字符中间。
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end) = find_close(text, i + 2) {
                let inner = &text[i + 2..i + 2 + end];
                let token = &text[i..i + 2 + end + 2];
                if !is_word(inner) && ctx.seen_bad_tokens.insert(token.to_string()) {
                    let d = if inner.starts_with('$') {
                        // Postman 动态变量在本仓库没有对应物，会原样留在 URL 里。
                        Diagnostic::info(
                            ptr,
                            "Postman 动态变量在本仓库没有对应物，发送时不会被替换",
                        )
                    } else {
                        Diagnostic::warn(
                            ptr,
                            "变量名含替换器不认的字符（只认字母数字与下划线），发送时不会被替换",
                        )
                    };
                    ctx.diagnostics.push(d.dropping(token));
                }
                i += 2 + end + 2;
                continue;
            }
        }
        i += 1;
    }
}

/// `}}` 相对 `from` 的偏移。
fn find_close(s: &str, from: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            return Some(i - from);
        }
        i += 1;
    }
    None
}
