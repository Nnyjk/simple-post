//! 把 token 挂到一份已解析好的请求上。

use crate::domain::{OAuth2Auth, OAuth2TokenLocation};
use crate::url::percent_encode;

use super::OAuth2Token;

/// `Authorization` 的完整取值：`<token_type> <access_token>`（RFC 6749 §7.1）。
///
/// `token_type` 去掉首尾空白后透传 IdP 回的值：它多数是 `Bearer`，少数是 `MAC` / `Token`，
/// 由服务端定义，替它改成 `Bearer` 会打错请求。去空白后是空串则只给裸 token。
pub fn auth_header_value(token: &OAuth2Token) -> String {
    let ty = token.token_type.trim();
    if ty.is_empty() {
        token.access_token.clone()
    } else {
        format!("{ty} {}", token.access_token)
    }
}

/// query 位置的 token 取值。与 header 档不同：query 位置按 RFC 6749 §8.3 只带裸
/// `access_token`，**不带** `token_type`——那会变成另一个参数值。
pub fn auth_query_value(token: &OAuth2Token) -> String {
    token.access_token.clone()
}

/// token 该放哪（`tokenLocation` 为 query 时调用方改走 URL 追加）。
pub fn goes_in_query(cfg: &OAuth2Auth) -> bool {
    cfg.token_location == OAuth2TokenLocation::Query
}

/// 把 token 挂到一份**已解析好**的请求上，就地改 `headers` / `url`。
///
/// 放在 core 而非各调用方，是为了让「query 位置用 `&` 还是 `?`」「参数名空白时回落」
/// 这些只有一处能定对的细节不被复制两份。渲染进程另有一份 TS 实现（`src/lib/oauth2.ts`），
/// 那是跨语言无法共享的部分，本函数刻意保持「纯数据 → 纯数据」。
///
/// 用户手写过 `Authorization` 时不覆盖：那与
/// `variable_fill::build_headers` 对 bearer / basic 的处理一致，也免掉「两个
/// Authorization」—— 服务端看到的是列表而非覆盖。
///
/// query 档的 token 用 [`crate::url::percent_encode`] 编码，与渲染进程那份 TS 实现
/// 同用一套字符集（JS 的 `encodeURIComponent`）；两边编码不同则同一个 token 在界面
/// 与 MCP 侧会落到两个 URL 上。
pub fn inject(
    cfg: &OAuth2Auth,
    token: &OAuth2Token,
    headers: &mut Vec<(String, String)>,
    url: &mut String,
) {
    if goes_in_query(cfg) {
        let glue = if url.contains('?') { '&' } else { '?' };
        *url = format!(
            "{url}{glue}{}={}",
            percent_encode(cfg.token_param_or_default()),
            percent_encode(&auth_query_value(token))
        );
        return;
    }
    if headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("authorization"))
    {
        return;
    }
    headers.push(("Authorization".to_string(), auth_header_value(token)));
}
