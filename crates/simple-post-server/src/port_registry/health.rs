//! GUI 用来发现「崩溃但留下端口文件」的 daemon 的 HTTP 探测。
//!
//! 路径写死为 `/mcp/healthz`：daemon 即使在自定义挂载前缀下也保留这个别名，改名会
//! 悄悄打断 supervisor 的存活探测。

use std::time::Duration;

/// 监管者用来发现「崩溃但留下端口文件」的 daemon 的 HTTP 探测。2 秒超时——既能跨过
/// 启动时 CPU 繁忙的瞬间，又不至于让渲染进程在启动画面上明显卡住。
///
/// `host` 是 daemon 实际绑定的那个地址。探测**先走回环**：daemon 绑 `0.0.0.0` 时回环
/// 也在监听范围内，而绑具体网卡地址（如 `192.168.12.81`）时回环不通，得再试那个地址
/// 本身。只探回环会把「绑了具体地址的 daemon」误判成起不来。
pub async fn healthz(port: u16, host: &str) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(target: "port_registry", "healthz client build: {e}");
            return false;
        }
    };
    let mut candidates = vec!["127.0.0.1".to_string()];
    let trimmed = host.trim();
    if !trimmed.is_empty() && trimmed != "127.0.0.1" && trimmed != "0.0.0.0" {
        candidates.push(trimmed.to_string());
    }
    for candidate in candidates {
        let url = format!("http://{candidate}:{port}/mcp/healthz");
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => return true,
            Ok(_) => {}
            Err(e) => {
                tracing::debug!(target: "port_registry", "healthz {url}: {e}");
            }
        }
    }
    false
}
