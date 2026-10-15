//! 按流程检查必填项，并给出该流程特有的那条错误。

use crate::domain::{OAuth2Auth, OAuth2GrantType};
use crate::error::AppError;
use crate::http_executor;

use super::parse::extract_code;

/// 按流程检查必填项。**刻意不在反序列化期拒绝**——`auth_json` 里有四年的 `'{}'` 与
/// `bearer` 行，那些必须继续读得出来（见 [`OAuth2Auth`] 的模块注释）。缺字段推迟到
/// 真正要用 token 时才报，这时信息才指向「你少了哪个」。
pub fn validate(cfg: &OAuth2Auth) -> Result<(), AppError> {
    match cfg.grant_type {
        OAuth2GrantType::Implicit => {
            if cfg.authorization_url.trim().is_empty() {
                return Err(AppError::Validation(
                    "implicit 流程需要 Authorization URL".into(),
                ));
            }
        }
        OAuth2GrantType::ClientCredentials => {
            require_token_url(cfg)?;
            if cfg.client_id.trim().is_empty() {
                return Err(AppError::Validation(
                    "client_credentials 流程需要 Client ID".into(),
                ));
            }
        }
        OAuth2GrantType::Password => {
            require_token_url(cfg)?;
            if cfg.client_id.trim().is_empty() {
                return Err(AppError::Validation("password 流程需要 Client ID".into()));
            }
            // 密码允许为空：是否接受空密码是 IdP 的决定，不该由本应用替它答。
            if cfg.username.trim().is_empty() {
                return Err(AppError::Validation("password 流程需要 Username".into()));
            }
        }
        OAuth2GrantType::AuthorizationCode => {
            require_token_url(cfg)?;
            if cfg.client_id.trim().is_empty() {
                return Err(AppError::Validation(
                    "authorization_code 流程需要 Client ID".into(),
                ));
            }
            if extract_code(&cfg.auth_code).is_empty() {
                return Err(AppError::Validation(
                    "authorization_code 流程需要授权码：请在浏览器登录后，把回调 URL \
                     或其中的 code 粘进「授权码」字段"
                        .into(),
                ));
            }
        }
    }
    Ok(())
}

fn require_token_url(cfg: &OAuth2Auth) -> Result<(), AppError> {
    if cfg.token_url.trim().is_empty() {
        return Err(AppError::Validation("需要 Token URL".into()));
    }
    match http_executor::check_url(
        cfg.token_url.trim(),
        http_executor::OutboundPolicy::default(),
    ) {
        http_executor::UrlVerdict::Allowed => Ok(()),
        // 出站策略拒绝原样透传：那是运维设的开关，用户需要看到**为什么**发不出去。
        http_executor::UrlVerdict::Refused(reason) => Err(AppError::Validation(reason)),
    }
}

pub(super) fn implicit_needs_manual() -> AppError {
    AppError::Validation(
        "implicit 流程没有 token 端点：请在浏览器完成授权后，把整条回调 URL 粘进 \
         OAuth2 配置再点「保存 Token」"
            .into(),
    )
}
