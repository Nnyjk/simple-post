// crates/simple-post-mcp/src/main.rs
//
// Standalone MCP entry point. This is currently a placeholder — commit 3
// only extracts the workspace structure. The real CLI with
// `--url` / `--password` / `--transport=stdio|http` lands in commit 4.
//
// The new binary name is `simple-post-mcp` (replacing the previous
// `simple-post-mcp-proxy`); see the BREAKING section of the commit
// 3 message for migration steps.

fn main() {
    eprintln!(
        "simple-post-mcp: standalone binary not yet wired (commit 4). \
         See crates/simple-post-mcp/README.md for the migration plan."
    );
    std::process::exit(2);
}