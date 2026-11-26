# 交换格式与导入导出

> 从 `AGENTS.md` 拆出的按需文档，原文逐字保留。只有动到本域时才需要读它。

**Interchange: `ir` 不是 `domain` 的别名。** 三条边性质不同：`format → ir` **有损**（故必须走 `Diagnostic`，不能静默丢）；`ir → domain` 无损全函数（补 id / `root_id` / 时间戳）；`domain → ir` 是投影（host 不在 endpoint 上，导出得自己选环境）。IR 必须自有的三条理由：预览阶段节点**还没有 id**、`domain` 无处安放诊断、导出需跨 `nodes`/`endpoint_details`/`projects`/`environments` 四表。指纹去重算在 IR 上，故两种导入器互相认得。**不要**为它加 `SCHEMA_VERSION`。

**Interchange 的 `IrNode.key` 是「IR 还没成为节点」的那一半身份**：导入 = 源格式的位置指针（与 `Diagnostic::pointer` 同一套寻址纪律），导出 = `nodes.id`。它是勾选集的成员身份，也是跨两侧引用一个条目的唯一句柄。`#[serde(default)]` 保证没有 `key` 的旧 IR 仍能反序列化。

**The transfer dialog is one component, not two.** Import and export share `TransferDialog` / `TransferTree` / `TransferNode` / `TransferEndpoint`; the modes differ in the source picker, two export-only settings, and the confirming action. Don't fork the tree or the types per mode — a shared filename is not a shared component. Its state lives in the `useTransfer` hook, **not** a Zustand slice: only `ProjectTree` can open it, so there is no second reader.

**Exported OpenAPI is checked by two outsiders, not by our own parser.** The importer in `simple-post-interchange` is ours too, so it is permanently lenient about its own generator's output — a missing `responses` yields no error, just one silently-absent field in an already-invalid document. So: `openapiv3` (dev-dep) validates against the official model inside `cargo test`, and Redocly (`npm run lint:openapi`, wired into the CI web job) checks rules. **Both read `crates/simple-post-interchange/fixtures/sample-exchange.json`** — the Rust test and `examples/export_sample.rs` start from the same tree, so two hand-written specimens would each cover half of it. `npm run lint:openapi` regenerates the document through the cargo example rather than committing a snapshot, because snapshots go stale. Rule opt-outs live in `redocly.yaml` and each one states why; silencing a rule is not how the next finding gets fixed.

**Export save is native-only, and the split is deliberate.** Tauri goes through `tauri-plugin-dialog` + `tauri-plugin-fs` (dynamically imported — they have no browser implementation, so a static import would make `serve-web` bundle dead Tauri code and throw at runtime); the browser falls back to a Blob + `<a download>`. The `fs` capability is granted **per verb and scoped to `$HOME/**/*`** — `fs:default` would open read and write over the whole user directory for a feature that writes one text file.
