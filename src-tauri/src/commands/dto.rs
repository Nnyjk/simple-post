// ---------------------------------------------------------------------------
// Create-options and move-target DTOs. Patch types live in `crate::domain`
// and are pulled in via `use crate::domain::*`.
//
// The definitions moved to `simple_post_server::dto` with the rest of the
// headless host. They were never Tauri-specific — plain
// `Serialize`/`Deserialize` shapes — but living in this crate put every
// one of them in the dependency cone of the GUI runtime, which is why
// `web_api` had to reach through `crate::commands::…` for a struct.
//
// Re-exported wholesale so every existing `crate::commands::dto::Foo`
// and `crate::commands::Foo` path below keeps resolving to the same
// types. This module is a compatibility surface, not a definition site.
// ---------------------------------------------------------------------------

pub use simple_post_server::dto::*;
