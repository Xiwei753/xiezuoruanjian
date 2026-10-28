use std::sync::Arc;

use rmcp::{model::ErrorCode, ErrorData};
use writer_core::api::service::WriterCoreApi;
use writer_core::api::WriterError;
use writer_core::writing_stats::{with_mutation_context, MutationContext};

/// Shared Core handle for all MCP tools.
///
/// Tool handlers never construct mutation contexts themselves: every write goes
/// through [`McpContext::write`], which pins the actor to `Ai` and the
/// entrypoint to `mcp`. Tool parameters therefore cannot claim another actor.
#[derive(Clone)]
pub struct McpContext {
    api: Arc<WriterCoreApi>,
}

impl McpContext {
    pub fn new(api: WriterCoreApi) -> Self {
        Self { api: Arc::new(api) }
    }

    pub fn api(&self) -> &WriterCoreApi {
        &self.api
    }

    /// Runs a Core mutation as an MCP-originated AI edit.
    pub fn write<T>(
        &self,
        operation: impl FnOnce(&WriterCoreApi) -> Result<T, WriterError>,
    ) -> Result<T, ErrorData> {
        let result = with_mutation_context(MutationContext::mcp(), || operation(&self.api));
        result.map_err(api_error)
    }
}

/// Runs a read-only Core query and maps its error to an MCP error response.
pub fn read<T>(operation: impl FnOnce() -> Result<T, WriterError>) -> Result<T, ErrorData> {
    operation().map_err(api_error)
}

pub fn api_error(error: WriterError) -> ErrorData {
    ErrorData::new(ErrorCode::INTERNAL_ERROR, error.to_string(), None)
}

pub fn invalid_params(message: impl Into<String>) -> ErrorData {
    ErrorData::invalid_params(message.into(), None)
}

pub fn to_json<T: serde::Serialize>(
    value: &T,
) -> Result<rmcp::handler::server::wrapper::Json<serde_json::Value>, ErrorData> {
    let value = serde_json::to_value(value)
        .map_err(|error| ErrorData::internal_error(format!("serialize response: {error}"), None))?;
    Ok(rmcp::handler::server::wrapper::Json(value))
}
