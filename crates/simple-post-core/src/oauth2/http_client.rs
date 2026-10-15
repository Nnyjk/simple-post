//! `oauth2` crate 的出站适配：把 crate 组好的请求送进 [`http_executor`]。
//!
//! # 为什么不用 crate 自带的 reqwest 客户端
//!
//! token 端点必须与普通请求走**同一道** [`http_executor`]：`OutboundPolicy`
//! （`SIMPLE_POST_BLOCK_PRIVATE_NET`）、超时、禁止重定向、体积上限都在那里。
//! crate 的 `reqwest` feature 会绕过这四样，于是 token 端点变成第二条出站通道 ——
//! 而它恰好带着 `client_secret` 与用户密码。所以 `Cargo.toml` 里
//! `default-features = false`，出站由本文件实现 [`AsyncHttpClient`]。
//!
//! # 非 2xx 为什么在这里就地变成错误
//!
//! crate 的 `check_response_status` 只认 200，且失败时要把 body 当 JSON 解析成
//! `error` / `error_description`。本项目的错误文案是中文的、且必须**裁剪**
//! （IdP 的 HTML 错误页整页塞进 `AppError` 会顶掉真正有用的那句），那套逻辑在
//! [`parse::error_response`]。所以非 2xx 在这里就地转成 [`AppError`] 返回：crate
//! 只看得到成功应答，它那套英文错误分支对本项目一行都不会执行。

use std::future::Future;
use std::pin::Pin;

use oauth2::{AsyncHttpClient, HttpRequest, HttpResponse};

use crate::error::AppError;
use crate::http_executor::{self, HttpRequestInput};

use super::parse::{error_response, normalize_success_body};
use super::{TOKEN_MAX_RESPONSE_KB, TOKEN_TIMEOUT_MS};

/// 无状态客户端。零大小，每次换 token 现建一个。
pub(super) struct ExecutorClient;

impl<'c> AsyncHttpClient<'c> for ExecutorClient {
    type Error = AppError;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, AppError>> + Send + 'c>>;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(exchange(request))
    }
}

async fn exchange(request: HttpRequest) -> Result<HttpResponse, AppError> {
    let (parts, body) = request.into_parts();
    // 非 UTF-8 的 body 直接报错，不静默发空 body：crate 组的是 form 编码的
    // UTF-8，真出现非 UTF-8 说明有别的东西坏了，而空 body 会让 IdP 报一个
    // 与真实原因无关的 400。
    let body = String::from_utf8(body)
        .map_err(|e| AppError::Http(format!("token 请求体不是 UTF-8: {e}")))?;
    let input = HttpRequestInput {
        method: parts.method.as_str().to_string(),
        url: parts.uri.to_string(),
        headers: parts
            .headers
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect(),
        body: Some(body),
        timeout_ms: TOKEN_TIMEOUT_MS,
        // 这次请求带着 `client_secret` / 用户密码 / 授权码，跟随重定向等于把它们再寄给
        // 一个未校验的目标——`OutboundPolicy` 只挡私有网段，挡不住 302 到另一个公网主机。
        // token 端点本就不该重定向（RFC 6749 §3.2 的 back-channel POST）。
        follow_redirects: false,
        max_response_size_kb: TOKEN_MAX_RESPONSE_KB,
    };

    let result = http_executor::execute(&input).await;
    let resp = result.response.ok_or_else(|| {
        AppError::Http(
            result
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "token 请求失败".into()),
        )
    })?;

    if !(200..300).contains(&resp.status) {
        return Err(error_response(resp.status, &resp.body, &resp.content_type));
    }

    let status = resp.status;
    let body = normalize_success_body(&resp.body, &resp.content_type);
    let mut out = HttpResponse::new(body.into_bytes());
    *out.status_mut() = oauth2::http::StatusCode::from_u16(status)
        .map_err(|e| AppError::Http(format!("token 响应状态码非法: {e}")))?;
    Ok(out)
}
