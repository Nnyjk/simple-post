//! 把 `fixtures/sample-exchange.json` 那棵树导出成 OpenAPI 3.0 文档。
//!
//! 存在的理由：Redocly（`npm run lint:openapi`）要的是**磁盘上的一份文档**，而产物是
//! Rust 生成的。与其提交一份会长过期的快照，不如现场生成——这份夹具与
//! `src/openapi_model_test.rs` 读的是**同一个文件**，故 linter 看到的与 `cargo test`
//! 校验过的是同一棵树。
//!
//! 夹具刻意覆盖导出器的每一处分支：八个合法动词、集合外的动词、四种 body mode、
//! 三种可映射的鉴权、嵌套集合、路径参数、配了服务与没配服务的两个模块。
//!
//! ```text
//! cargo run -q -p simple-post-interchange --example export_sample -- out.yaml
//! cargo run -q -p simple-post-interchange --example export_sample -- out.json --json
//! cargo run -q -p simple-post-interchange --example export_sample -- --ir out.ir.json
//! ```
//!
//! `--ir` 落的是 IR 本身。改了 `IrEndpoint` 的字段却忘了同步夹具时，那条命令会报出
//! 缺哪个字段——否则夹具会安静地按 `#[serde(default)]` 补出零值，而那份文档看起来
//! 仍然完全正常。

use std::process::ExitCode;

use simple_post_interchange::ir::Exchange;
use simple_post_interchange::openapi::{to_document, DocFormat};

const FIXTURE: &str = include_str!("../fixtures/sample-exchange.json");

fn main() -> ExitCode {
    let mut want_json = false;
    let mut want_ir = false;
    let mut out_path = None;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--json" => want_json = true,
            "--ir" => want_ir = true,
            "--help" | "-h" => {
                println!(
                    "usage: export_sample [--json] [--ir] <out-path>\n  \
                     --json  产出 JSON 而不是 YAML\n  \
                     --ir    产出 IR 夹具本身（调试夹具失配用）"
                );
                return ExitCode::SUCCESS;
            }
            other => out_path = Some(other.to_string()),
        }
    }

    let Some(out_path) = out_path else {
        eprintln!("export_sample: 缺少输出路径（见 --help）");
        return ExitCode::FAILURE;
    };

    let exchange: Exchange = match serde_json::from_str(FIXTURE) {
        Ok(ex) => ex,
        Err(e) => {
            // 夹具与 IR 类型脱节了。这是**代码**问题不是文档问题，故别把它写成
            // 「解析失败」——那句话会让人去查 OpenAPI，而真正的原因是夹具少字段。
            eprintln!(
                "export_sample: fixtures/sample-exchange.json 已与 IR 类型脱节：{e}\n\
                 提示：先跑 `--ir` 看这份夹具到底有哪些字段，再补齐。"
            );
            return ExitCode::FAILURE;
        }
    };

    let text = if want_ir {
        serde_json::to_string_pretty(&exchange).expect("IR 序列化")
    } else {
        let format = if want_json {
            DocFormat::Json
        } else {
            DocFormat::Yaml
        };
        let doc = match to_document(&exchange, format) {
            Ok(doc) => doc,
            Err(e) => {
                eprintln!("export_sample: 生成失败：{e}");
                return ExitCode::FAILURE;
            }
        };
        // 跳过的不该静默。写文档的人要知道这份产物少了一块。
        for d in &doc.diagnostics {
            eprintln!("[{:?}] {} {}", d.severity, d.pointer, d.message);
        }
        doc.content
    };

    if let Some(parent) = std::path::Path::new(&out_path).parent() {
        if !parent.as_os_str().is_empty() {
            // 调用方给的多半是 `.scratch/…`，而那目录是 gitignored 的，不保证存在。
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("export_sample: 建 {} 失败：{e}", parent.display());
                return ExitCode::FAILURE;
            }
        }
    }
    if let Err(e) = std::fs::write(&out_path, text) {
        eprintln!("export_sample: 写 {out_path} 失败：{e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
