//! `RequestSettingsPatch` 的两条契约：写错键名必须报错；读回来的一整份设置能原样 PATCH 回去。
//!
//! 它是五个补丁类型里唯一一个只有 `Deserialize` 的（Tauri 的 `request_settings_update` 只收
//! 局部更新），故身份字段不需要——[`RequestSettings`] 的四个字段名与补丁逐个一致。

use serde_json::json;

use super::RequestSettingsPatch;
use simple_post_core::domain::{RequestSettings, TlsSettings};

#[test]
fn a_misspelled_setting_is_rejected() {
    // `timeoutMS` 是很容易写错的那个键。静默丢掉未知键的话，调用方拿到 200、设置一字未改，
    // 而它以为改成功了。
    let err =
        serde_json::from_value::<RequestSettingsPatch>(json!({ "timeoutMS": 1_000 })).unwrap_err();
    assert!(err.to_string().contains("timeoutMS"), "got {err}");
}

#[test]
fn a_settings_read_back_can_be_written_back_as_a_patch() {
    let settings = RequestSettings {
        timeout_ms: 12_000,
        follow_redirects: false,
        max_response_size_kb: 900,
        tls: TlsSettings {
            skip_verify: true,
            ..TlsSettings::default()
        },
    };
    let patch: RequestSettingsPatch =
        serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
    assert_eq!(patch.timeout_ms, Some(12_000));
    assert_eq!(patch.follow_redirects, Some(false));
    assert_eq!(patch.max_response_size_kb, Some(900));
    assert_eq!(patch.tls.map(|t| t.skip_verify), Some(true));
}
