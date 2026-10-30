## 这个 PR 做了什么

<!-- 一两句话。改的是行为还是文档？如果是文档，直接写清楚哪个陈述不成立了。 -->

Closes #

## 变更清单

<!-- 按文件列，标注新增 / 修改 / 删除。纯重构也要说清楚「逻辑未变」。 -->

| 文件 | 动作 | 说明 |
|---|---|---|
|  |  |  |

## 红线自查

<!-- 涉及下面任何一条，先读 .github/CONTRIBUTING.md 对应段落再勾。 -->

- [ ] schema 改动要么走 `db::ADDED_COLUMNS`（纯增量，不 bump `SCHEMA_VERSION`），要么是一次明确的 bump（bump 会删掉每个用户的 store.db）
- [ ] 新增 MCP 工具的话，`tool_table()` 与 `REQUIRED_CAPABILITY` **两处**都改了，且 Capability 正确（`Send` 只有 `full_access` 覆盖）
- [ ] 没有引入 `check_permission(..).await.ok()`（会把 `PermissionDenied` 吞掉，闸门形同不存在）
- [ ] 需要写项目 id 的处理器解析了真实归属项目，没有传字面量 `"global"`
- [ ] 新增后端命令的话，五处协同都改了：`commands/` handler、`lib.rs` 注册、`COMMANDS`、`web_api/` 路由、`WEB_ROUTES`
- [ ] 两边都需要的 DTO 放在 `crates/simple-post-server/src/dto.rs`，没有放回 `commands/`
- [ ] 没有给 `vite.config.ts` 加 `base`，没有加 `<base href>`，nginx 侧没有剥离前缀
- [ ] 没有删除或放宽 `mcp/mod_test.rs` / `web_api/` 里的回归网测试
- [ ] CI 变更只减少「不相关的组合」，没有删除任何既有断言

## 验证

<!-- 跑了什么就写什么。没跑就写「未执行」——不要写成通过。 -->

```
npm run check
cargo test --workspace
```

- [ ] `npm run check` 通过
- [ ] `cargo test --workspace` 通过
- [ ] 容器相关改动：`npm run test:web:auth` 与 `npm run test:web:basepath` 通过
- [ ] MCP 相关改动：新增/修改的工具在两个档位下的 `tools/list` 都符合预期

## 截图

<!-- UI 改动必填，深浅色各一张。纯后端/文档改动删掉本节。 -->

| 浅色 | 暗色 |
|---|---|
|  |  |

## 风险与回滚

<!-- 出问题怎么发现、怎么退。引入新依赖或改 schema 迁移路径时必填。 -->