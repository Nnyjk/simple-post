//! 导出：文档形状、动词过滤、`servers` 不回退、`securitySchemes` 归并。

use serde_json::Value;
use simple_post_core::domain::HttpMethod;

use super::{to_document, DocFormat};
use crate::ir::{Exchange, IrEndpoint, IrModule};

pub(super) fn ep(name: &str, method: HttpMethod, path: &str) -> IrEndpoint {
    IrEndpoint::new(name, method, path)
}

fn module(name: &str, endpoints: Vec<IrEndpoint>) -> IrModule {
    IrModule {
        name: name.into(),
        endpoints,
        ..Default::default()
    }
}

pub(super) fn one_module(name: &str, endpoints: Vec<IrEndpoint>) -> Exchange {
    Exchange {
        project_name: "demo".into(),
        modules: vec![module(name, endpoints)],
        ..Default::default()
    }
}

pub(super) fn doc_of(ex: &Exchange) -> Value {
    let out = to_document(ex, DocFormat::Json).expect("导出应成功");
    serde_json::from_str(&out.content).expect("产物应是合法 JSON")
}
