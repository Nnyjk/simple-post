//! `crate::tls` 的行为：全局默认与端点覆盖怎么合成，以及这组设置怎么落到 `reqwest` 上。
//!
//! 夹具是本仓库自签的测试证书（`tls_testdata/` 下），不是任何真实凭据。

use super::*;
use crate::domain::TlsSettings;

// 夹具——自签证书，由 .NET 的 `CertificateRequest` 生成，仅供本测试用。

const CA_CERT: &str = include_str!("tls_testdata/ca-cert.pem");
const CA_BUNDLE: &str = include_str!("tls_testdata/ca-bundle.pem");
const CLIENT_CERT: &str = include_str!("tls_testdata/client-cert.pem");
const CLIENT_KEY: &str = include_str!("tls_testdata/client-key.pem");

/// 施加设置并把客户端丢掉：这些用例只关心「成不成功」。
///
/// 顺带真的建一次客户端，故「设置被接受但客户端建不出来」这种情形也会被这里逮到。
fn apply(tls: &TlsSettings) -> Result<(), AppError> {
    apply_to_builder(reqwest::Client::builder(), tls)?
        .build()
        .map(|_| ())
        .map_err(|e| AppError::Validation(format!("client build: {e}")))
}

/// 写一个临时文件，返回它的路径。
///
/// 不删：临时目录下的文件由系统清理，而删文件的失败会让测试变成 flaky。
fn temp_file(name: &str, contents: &str) -> String {
    let path = std::env::temp_dir().join(format!("simple-post-tls-test-{name}"));
    std::fs::write(&path, contents).expect("写临时文件");
    path.to_string_lossy().into_owned()
}

// 合成语义。

#[test]
fn an_endpoint_without_its_own_settings_inherits_the_global_ones() {
    let global = TlsSettings {
        skip_verify: true,
        ca_certificate_path: Some("C:/certs/ca.pem".into()),
        client_certificate_path: None,
        client_key_path: None,
    };
    assert_eq!(resolve(&global, None), global);
}

#[test]
fn the_endpoints_own_settings_replace_the_global_ones_as_a_whole() {
    let global = TlsSettings {
        skip_verify: true,
        ca_certificate_path: Some("C:/certs/ca.pem".into()),
        client_certificate_path: Some("C:/certs/client.pem".into()),
        client_key_path: Some("C:/certs/client.key".into()),
    };
    // 端点这一份里什么都没有，但它**整体**取代全局那份：端点一旦给了设置，
    // 「端点只想改一项、其余跟着全局」就不是这个模型能表达的。
    let endpoint = TlsSettings::default();
    let merged = resolve(&global, Some(&endpoint));
    assert_eq!(merged, endpoint);
    assert!(!merged.skip_verify);
    assert_eq!(merged.ca_certificate_path, None);
}

#[test]
fn an_endpoint_with_its_own_settings_keeps_them() {
    let global = TlsSettings::default();
    let endpoint = TlsSettings {
        skip_verify: false,
        ca_certificate_path: Some("C:/certs/other-ca.pem".into()),
        client_certificate_path: None,
        client_key_path: None,
    };
    assert_eq!(resolve(&global, Some(&endpoint)), endpoint);
}

// 施加到 reqwest。

#[test]
fn the_default_settings_leave_the_client_untouched() {
    apply(&TlsSettings::default()).expect("默认设置不该报错");
}

#[test]
fn skipping_the_verification_is_accepted() {
    let tls = TlsSettings {
        skip_verify: true,
        ..TlsSettings::default()
    };
    apply(&tls).expect("跳过校验不该报错");
}

#[test]
fn a_custom_ca_certificate_is_accepted() {
    let tls = TlsSettings {
        ca_certificate_path: Some(temp_file("ca.pem", CA_CERT)),
        ..TlsSettings::default()
    };
    apply(&tls).expect("CA 证书该被接受");
}

#[test]
fn a_ca_bundle_with_several_certificates_is_accepted() {
    let tls = TlsSettings {
        ca_certificate_path: Some(temp_file("ca-bundle.pem", CA_BUNDLE)),
        ..TlsSettings::default()
    };
    apply(&tls).expect("多张 CA 证书该被接受");
}

/// CA 这一半的补边界必须有回归网。
///
/// 夹具 `ca-bundle.pem` 里是两张证书，且**末尾没有换行**。不补边界时 reqwest 只解析出
/// **一张** —— 另一张被静默丢出信任链（不报错，握手时找不到签发者）。故这里直接断言
/// 张数：只断言「被接受」的话，去掉补边界这一步用例照样变绿。
#[test]
fn every_certificate_in_a_bundle_survives_the_boundary_normalisation() {
    let pem = normalise_pem_boundaries(CA_BUNDLE.as_bytes());
    let certs = reqwest::Certificate::from_pem_bundle(&pem).expect("bundle 该解析得动");
    assert_eq!(
        certs.len(),
        2,
        "bundle 里两张证书都要在，补边界不能只保住第一张"
    );
}

#[test]
fn a_missing_ca_file_is_reported_with_its_path() {
    let tls = TlsSettings {
        ca_certificate_path: Some("C:/definitely/missing/ca.pem".into()),
        ..TlsSettings::default()
    };
    let err = apply(&tls).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("ca.pem"),
        "错误里得指出是哪个文件读不到，got: {msg}"
    );
}

#[test]
fn a_ca_file_without_a_certificate_in_it_is_rejected() {
    let tls = TlsSettings {
        ca_certificate_path: Some(temp_file("empty-ca.pem", "not a certificate\n")),
        ..TlsSettings::default()
    };
    let err = apply(&tls).unwrap_err();
    assert!(
        err.to_string().contains("没有可用的证书"),
        "空文件该被说清，got: {err}"
    );
}

#[test]
fn a_client_certificate_and_a_separate_key_form_a_pair() {
    let tls = TlsSettings {
        client_certificate_path: Some(temp_file("client-cert.pem", CLIENT_CERT)),
        client_key_path: Some(temp_file("client-key.pem", CLIENT_KEY)),
        ..TlsSettings::default()
    };
    apply(&tls).expect("证书与私钥该凑成一对");
}

#[test]
fn a_certificate_and_a_key_whose_files_do_not_end_with_a_newline_form_a_pair() {
    // 回归点：用户给的两个文件末尾常常没有换行，两个文件直接首尾相接时，私钥的 BEGIN
    // 标记就与证书的 END 标记粘在同一行里。`reqwest` 的解析器把整行当成证书那个区块的
    // 结尾，于是**私钥那段被整段跳过**，报的错是「private key or certificate not found」。
    // 本仓库的夹具末尾本来就没有换行，故这两个用例合起来钉住了这条修法。
    assert!(
        !CLIENT_CERT.ends_with('\n') && !CLIENT_KEY.ends_with('\n'),
        "夹具要保持「末尾没有换行」的样子，否则这个用例测不到要修的那条路径"
    );
    let tls = TlsSettings {
        client_certificate_path: Some(temp_file("client-cert-nolf.pem", CLIENT_CERT)),
        client_key_path: Some(temp_file("client-key-nolf.pem", CLIENT_KEY)),
        ..TlsSettings::default()
    };
    apply(&tls).expect("两份末尾都没有换行的文件也该凑成一对");
}

#[test]
fn a_client_certificate_and_its_key_in_one_file_form_a_pair() {
    let merged = format!("{CLIENT_CERT}{CLIENT_KEY}");
    let tls = TlsSettings {
        client_certificate_path: Some(temp_file("client-merged.pem", &merged)),
        client_key_path: None,
        ..TlsSettings::default()
    };
    apply(&tls).expect("合并在一个文件里的证书与私钥该被接受");
}

#[test]
fn a_client_certificate_without_a_key_is_rejected() {
    let tls = TlsSettings {
        client_certificate_path: Some(temp_file("cert-only.pem", CLIENT_CERT)),
        client_key_path: None,
        ..TlsSettings::default()
    };
    let err = apply(&tls).unwrap_err();
    assert!(
        err.to_string().contains("无法组成一对"),
        "缺私钥该被说清，got: {err}"
    );
}

#[test]
fn a_key_without_a_certificate_is_rejected() {
    let tls = TlsSettings {
        client_certificate_path: None,
        client_key_path: Some(temp_file("key-only.pem", CLIENT_KEY)),
        ..TlsSettings::default()
    };
    let err = apply(&tls).unwrap_err();
    assert!(
        err.to_string().contains("没有客户端证书路径"),
        "只给私钥该被说清，got: {err}"
    );
}

#[test]
fn a_blank_path_counts_as_not_filled_in() {
    // 界面上清空输入框留下的就是空白串。它不能被当成「有一个叫空白的文件」。
    let tls = TlsSettings {
        skip_verify: false,
        ca_certificate_path: Some("   ".into()),
        client_certificate_path: Some("".into()),
        client_key_path: Some("  ".into()),
    };
    apply(&tls).expect("空白路径该被当成没填");
    assert!(!tls.has_ca_certificate());
    assert!(!tls.has_client_certificate());
}

// 路径字段的取值校验。

#[test]
fn an_ordinary_path_passes_the_validation() {
    let tls = TlsSettings {
        ca_certificate_path: Some("C:/certs/company-ca.pem".into()),
        client_certificate_path: Some("C:/certs/client.pem".into()),
        client_key_path: Some("C:/certs/client.key".into()),
        ..TlsSettings::default()
    };
    tls.validate().expect("普通路径该通过校验");
}

#[test]
fn a_blank_path_passes_the_validation() {
    // 空白串是「没填」，不是「一个奇怪的文件名」。
    let tls = TlsSettings {
        ca_certificate_path: Some("   ".into()),
        client_certificate_path: Some(String::new()),
        client_key_path: None,
        ..TlsSettings::default()
    };
    tls.validate().expect("空白路径该通过校验");
}

#[test]
fn a_pasted_certificate_body_is_rejected_instead_of_being_stored() {
    // 回归点：用户把私钥正文粘进「客户端私钥」输入框。那段文本会原样落进
    // `preferences` 的 `request_settings` 行或 `endpoint_details.tls_json`，而
    // `GET /api/preferences` 把整行原样返回 —— 「证书只存路径，不存内容」要在写入口
    // 守住，否则这条不变量在用户手滑时就没了。
    let tls = TlsSettings {
        client_key_path: Some(format!("{}\n", CLIENT_KEY)),
        ..TlsSettings::default()
    };
    let err = tls.validate().unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("客户端私钥"),
        "报错要点名是哪一个字段，got: {msg}"
    );
    assert!(
        msg.contains("不是文件路径"),
        "报错要说清填错了什么，got: {msg}"
    );
}

#[test]
fn a_pasted_ca_body_is_rejected_too() {
    let tls = TlsSettings {
        ca_certificate_path: Some(CA_CERT.to_string()),
        ..TlsSettings::default()
    };
    let err = tls.validate().unwrap_err();
    assert!(
        err.to_string().contains("自定义 CA 证书"),
        "报错要点名是哪一个字段，got: {err}"
    );
}

#[test]
fn a_path_with_a_line_break_in_it_is_rejected() {
    let tls = TlsSettings {
        client_certificate_path: Some("C:/certs/client\n.pem".into()),
        ..TlsSettings::default()
    };
    let err = tls.validate().unwrap_err();
    assert!(
        err.to_string().contains("不能有换行"),
        "路径里带换行该被说清，got: {err}"
    );
}
