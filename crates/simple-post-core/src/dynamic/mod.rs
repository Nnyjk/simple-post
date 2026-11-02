//! 动态变量的解析：取数、抽取、缓存命中、single-flight。
//!
//! 一条动态变量的声明是「变量名 → 数据源 → 抽取规则 → 缓存时长」。消费端点的请求体里写
//! `{{sns}}`，发送时程序按本模块解析出值，并进变量池。
//!
//! ## 为什么解析在「替换」之前
//!
//! 变量池的合并规则有两条子规则：动态变量的值优先于同名环境变量；被停用的环境变量不影响
//! 动态变量。这两条只有在替换之前并池才成立——替换一旦发生，池里已经没有同名变量的概念了。
//! 故本模块的产出是**一个变量名到值的映射**，由调用方在替换之前并进池。
//!
//! ## 取数上下文由定义自己决定
//!
//! 定义挂在项目里，故同一个 `{{name}}` 只在消费方那个项目里指向一条定义。取数要用的
//! 项目、环境与模块仍然全部由那条定义推导（见 [`source_context`]），而不是由「谁在
//! 消费这个变量」决定：数据源端点落在哪个项目就用哪个项目的模块，定义点名的
//! 环境就是那个环境，没点名就用该项目的第一个可用环境。
//!
//! ## 分层
//!
//! 本模块住在 core，因为它只需要 core 的 db / http_executor / template。但「引用端点」形态的
//! 数据源要用 `variable_fill::resolve_endpoint` 组装请求，那个函数住在 mcp crate，而 core
//! 不能依赖 mcp。故本模块定义 [`SourceFetcher`] 抽象，由 mcp crate 提供实现——
//! 两侧 transport 共用同一份取数逻辑。
//!
//! ## 取数失败不让整次发送失败
//!
//! 脚本失败不影响请求的成败（见 `crate::script::send_with_scripts`），取数失败同样不影响。
//! 请求发出去，服务端多半会返回一个明确的业务错误，那比客户端提前放弃更有信息量。
//! 退化顺序：覆盖值 → 过期缓存值（仅当 `stale_fallback` 为真） → 按缺失处理。
//!
//! ## 模块地图
//!
//! - `merge.rs`：变量池的合并规则
//! - `serialize.rs`：值的序列化（`value_mode` 的形状）
//! - `names.rs`：请求引用了哪些 `{{name}}`
//! - `cache.rs`：缓存决定
//! - `extract.rs`：按 JSONPath 抽值
//! - `source.rs`：定义取数用的环境 / 模块
//! - `fetcher.rs`：取数器抽象与一次解析的上下文
//! - `flight.rs`：single-flight 注册表
//! - `resolve.rs`：编排——取数、抽取、落缓存
//! - `transform.rs`：抽取之后跑处理脚本，用它的返回值替换抽取结果

mod cache;
mod extract;
mod fetcher;
mod flight;
mod merge;
mod names;
mod resolve;
mod serialize;
mod source;
mod transform;

// 公开接口：定义在按职责分开的子模块里，这里重新导出，故 `dynamic::merge_pool` 等旧路径不变。
pub use cache::{decide, Decision};
pub use extract::extract_value;
pub use fetcher::{RawSource, ResolveContext, SourceFetcher};
pub use merge::merge_pool;
pub use names::{list_names, TemplateParts};
pub use resolve::{resolve_definition, resolve_names, MAX_VALUE_BYTES};
pub use serialize::{parse_raw, serialize_value};
pub use source::{source_context, SourceContext};
pub use transform::apply as apply_transform;

// `SourceFetcher` 的签名里出现 `DynamicError`，测试模块靠 `use super::*` 拿到这个类型。
// 这个重导出只在测试构建里存在，生产构建的公开接口不含它。
#[cfg(test)]
pub use crate::domain::DynamicError;

// 测试。

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;

#[cfg(test)]
#[path = "names_test.rs"]
mod names_tests;

#[cfg(test)]
#[path = "resolve_parity_test.rs"]
mod parity_tests;
