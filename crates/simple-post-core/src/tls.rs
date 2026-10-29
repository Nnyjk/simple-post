//! HTTPS 出站：全局默认与端点覆盖怎么合成，以及这组设置怎么落到 `reqwest` 上。
//!
//! 本模块是这三件事的**唯一**实现处，故桌面端、`serve-web` 端与 MCP 端不会对「这次请求
//! 用哪一份证书、要不要跳过校验」各说各话。
//!
//! # 合成语义：端点的设置整体取代全局的设置
//!
//! 端点要么完全用自己的那份 HTTPS 设置，要么完全用全局那份。逐字段合并会让「端点只改
//! 一项、其余跟着全局」成为可能，而那要求每个字段都带「继承 / 自定义 / 清空」三态，
//! 界面上多出三倍的开关，且「端点清掉全局的 CA 路径」与「端点没配 CA 路径」两种情形
//! 在存储上无法区分。故不做。
//!
//! # 证书只存路径
//!
//! `TlsSettings` 里的三个路径字段指向 PEM 文件，文件内容不进数据库。私钥一旦进库，
//! `GET /api/preferences` 会把整行原样返回，SQLite 文件本身也是一份用户没预期的备份。
//!
//! 路径由**运行请求的那台机器**解析：桌面端是本机路径，`serve-web` / Docker 端是服务端
//! 容器里的路径。

use crate::db::DbPool;
use crate::domain::TlsSettings;
use crate::error::AppError;

// 合成。

/// 合成一次请求实际生效的 HTTPS 设置。
///
/// 端点的设置整体取代全局的设置：端点给了一份就用端点那份，没给就用全局那份。
///
/// 全局那份由调用方给：桌面端从设置面板的值来（它与库里那一行同源），MCP 端从库里读。
/// 故本函数是纯函数，故两侧对同一组输入得到同一份结果。
pub fn resolve(global: &TlsSettings, endpoint: Option<&TlsSettings>) -> TlsSettings {
    match endpoint {
        Some(t) => t.clone(),
        None => global.clone(),
    }
}

/// 全局默认的 HTTPS 设置（`preferences` 的 `request_settings` 行）。
///
/// 读不出来时退回缺省形态，而不是让请求失败：没配过证书时那一行本来就可能不存在，
/// 而读设置失败不该连带让真正的出站请求发不出去。
pub fn global_default(pool: &DbPool) -> TlsSettings {
    crate::db::with_conn(pool, crate::db::preferences::request_settings)
        .map(|s| s.tls)
        .unwrap_or_default()
}

// 施加到 reqwest。

/// 把 HTTPS 设置施加到 `reqwest` 的客户端构造器上。
///
/// `reqwest` 用的是 `rustls` 后端（见工作区 `Cargo.toml` 里 `reqwest` 的 `rustls-tls`
/// feature），故三件事各自对应一个 builder 方法：
///
/// * **跳过校验** → `danger_accept_invalid_certs` + `danger_accept_invalid_hostnames`。
///   两个都要开：reqwest 的 rustls 后端在证书校验被关掉时**整体换掉**校验器
///   （`client.rs` 里的 `NoVerifier`），只开主机名那一项会落到另一个分支——证书链照查、
///   主机名不查，与本开关宣称的「跳过校验」不是一回事。
/// * **自定义 CA** → `add_root_certificate`。一个 PEM 文件里可以含多张证书，逐张加进
///   信任链。
/// * **客户端证书** → `identity`。`reqwest` 的 `Identity::from_pem` 从同一段 PEM 里找出
///   私钥与证书，故证书与私钥分开存放在两个文件时，把两个文件的内容接在一起交给它。
///
/// **跳过校验时自定义 CA 不参与**：rustls 后端在跳过校验时用 `NoVerifier` 替换整个校验器，
/// 信任链（含加进去的自定义 CA）根本不参与。加证书这一步照做不误，只是结果没有被用到。
pub fn apply_to_builder(
    builder: reqwest::ClientBuilder,
    tls: &TlsSettings,
) -> Result<reqwest::ClientBuilder, AppError> {
    let mut builder = builder;

    if tls.skip_verify {
        builder = builder
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true);
    }

    if let Some(path) = non_empty(&tls.ca_certificate_path) {
        let pem = read_pem(path, "CA 证书")?;
        let pem = normalise_pem_boundaries(&pem);
        let certs = reqwest::Certificate::from_pem_bundle(&pem)
            .map_err(|e| AppError::Validation(format!("CA 证书文件 {path} 解析失败: {e}")))?;
        if certs.is_empty() {
            return Err(AppError::Validation(format!(
                "CA 证书文件 {path} 里没有可用的证书"
            )));
        }
        for cert in certs {
            builder = builder.add_root_certificate(cert);
        }
    }

    match (
        non_empty(&tls.client_certificate_path),
        non_empty(&tls.client_key_path),
    ) {
        (Some(cert_path), key_path) => {
            let mut pem = read_pem(cert_path, "客户端证书")?;
            // 两个路径都要出现在报错里：私钥文件选错（选了 DER 格式的 .p12、选了一个
            // 截断的密钥）时，`from_pem` 报的还是「private key or certificate not
            // found」，那句话单看只说明「这对材料凑不齐」。只点证书路径的话，用户看到
            // 一份**没问题**的证书被点名，而真正坏掉的那个文件从头到尾没出现。
            let materials = match key_path {
                Some(k) => format!("{cert_path}（客户端证书）与 {k}（客户端私钥）"),
                None => format!("{cert_path}（客户端证书，私钥要从这个文件里读）"),
            };
            if let Some(k) = key_path {
                pem.extend_from_slice(&read_pem(k, "客户端私钥")?);
            }
            let pem = normalise_pem_boundaries(&pem);
            let identity = reqwest::Identity::from_pem(&pem).map_err(|e| {
                AppError::Validation(format!(
                    "客户端证书材料无法组成一对（{materials}）: {e}。{}",
                    error_chain(&e)
                ))
            })?;
            builder = builder.identity(identity);
        }
        (None, Some(_)) => {
            return Err(AppError::Validation(
                "填了客户端私钥路径却没有客户端证书路径：客户端证书要连着私钥一起出示".into(),
            ));
        }
        (None, None) => {}
    }

    Ok(builder)
}

// 内部辅助。

/// 取一个非空的路径。空白串视同没填：界面上清空输入框留下的就是空白串。
fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// 读一个 PEM 文件。读不到时报出路径，让用户知道是哪个文件出了问题。
fn read_pem(path: &str, what: &str) -> Result<Vec<u8>, AppError> {
    std::fs::read(path).map_err(|e| AppError::Validation(format!("{what}文件 {path} 读不到: {e}")))
}

/// 把 PEM 的区块边界补齐：每个 `-----END <label>-----` 后面必须紧跟一个换行。
///
/// `reqwest` 的 PEM 解析器按行读区块。当某个区块的 END 标记后面没有换行，下一个区块的
/// BEGIN 标记就与它粘在同一行里，解析器把整行当成前一个区块的结尾，于是**后面那个区块
/// 被整段跳过**。实测：把「证书.pem」与「私钥.pem」直接首尾相接时，私钥那份读不到，
/// 报错是「private key or certificate not found」——而 PEM 本身完全合法。
///
/// 「证书与私钥分两个文件」正是最常见的 mTLS 材料形态，且用户给的文件末尾常常没有换行，
/// 故在读进来之后先把边界补齐。
///
/// 已在原缓冲区里带换行的地方不动，故这个函数不改变已有内容的字节。
fn normalise_pem_boundaries(pem: &[u8]) -> Vec<u8> {
    const END_MARKER: &[u8] = b"-----END ";

    let mut out = Vec::with_capacity(pem.len() + 32);
    let mut rest = pem;
    loop {
        let Some(pos) = find_bytes(rest, END_MARKER) else {
            out.extend_from_slice(rest);
            break;
        };
        out.extend_from_slice(&rest[..pos]);

        // END 标记的收尾是 5 个连字符：标签之后的第一处「-----」。
        let after = &rest[pos + END_MARKER.len()..];
        let Some(dashes) = find_bytes(after, b"-----") else {
            // 不是一条完整的 END 标记，就原样留着。
            out.extend_from_slice(&rest[pos..]);
            break;
        };
        let marker_end = pos + END_MARKER.len() + dashes + 5;
        out.extend_from_slice(&rest[pos..marker_end]);

        // 标记后面若没有换行，就补一个：后面粘着的内容要另起一行。
        let tail = &rest[marker_end..];
        if !tail.starts_with(b"\n") && !tail.starts_with(b"\r") {
            out.push(b'\n');
        }
        rest = tail;
    }
    out
}

/// 字节切片里第一个 `needle` 的位置。
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 错误链上的文字。
///
/// `reqwest` 的构造错误只说「builder error」，真正的原因（缺私钥、缺证书、PEM 不合法）
/// 在 source 链里。不把它翻出来，用户拿到的提示没法用来排查。
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut parts = Vec::new();
    let mut cur = e.source();
    while let Some(c) = cur {
        parts.push(c.to_string());
        cur = c.source();
    }
    if parts.is_empty() {
        return "错误链上没有更细的说明。PEM 里须含证书与私钥，或另填客户端私钥路径".into();
    }
    parts.join(" ← ")
}

// 测试。

#[cfg(test)]
#[path = "tls_test.rs"]
mod tests;
