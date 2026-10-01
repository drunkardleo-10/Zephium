//! A small MCP server over stdio for tests: `echo`, `add`, `send_note`,
//! `flood` and one resource.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    zephium_mcp::fixture::serve_stdio().await;
}
