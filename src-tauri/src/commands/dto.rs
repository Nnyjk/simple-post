// ---------------------------------------------------------------------------
// 创建选项与移动目标的 DTO。Patch 类型在 `crate::domain`，经 `use crate::domain::*` 进来。
//
// 定义在 `simple_post_server::dto`：这些形状从不是 Tauri 专属（只是普通
// `Serialize` / `Deserialize`），留在本 crate 会把每一个都拖进 GUI 运行时依赖锥，
// `web_api` 才不得不绕 `crate::commands::…` 拿结构体。
//
// 这里整体转发，只为保住既有的 `crate::commands::dto::Foo` 与 `crate::commands::Foo`
// 路径：本模块是兼容面，不是定义处。
// ---------------------------------------------------------------------------

pub use simple_post_server::dto::*;
