use rmcp::{
    ServerHandler,
    model::{Implementation, ServerCapabilities, ServerConfig},
    tool_handler,
};

use crate::context::McpContext;

/// MCP server exposing Sujian Core data over stdio.
///
/// The router and every tool implementation live in `tools.rs`; this type only
/// holds the shared Core context and the MCP server metadata.
#[derive(Clone)]
pub struct SujianMcpServer {
    context: McpContext,
}

impl SujianMcpServer {
    pub fn new(context: McpContext) -> Self {
        Self { context }
    }

    pub fn ctx(&self) -> &McpContext {
        &self.context
    }
}

#[tool_handler]
impl ServerHandler for SujianMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("sujian_mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(format!(
                "Sujian local writing workspace. Chapter body text is read-only. \
                 Starmaps, tags, object links and chapter text anchors are writable metadata; \
                 every write is validated against the authorized scope and audited after commit. {}",
                self.context.scope_summary()
            ))
    }
}
