//! 同步传输契约 — 将同步协议与 HTTP 执行分离。
//!
//! `SyncTransport` trait 定义了同步操作所需的网络能力边界。
//! Core 的同步引擎生成请求计划，平台层负责实际 HTTP 执行。
//!
//! ## 依赖方向
//!
//! ```text
//! writer_core 同步引擎 → SyncTransport trait → 平台 HTTP 实现
//! ```
//!
//! Core 不直接依赖 `reqwest`、`git2` 或平台 TLS 库。
//! 平台端注入具体的 HTTP 客户端实现。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportError {
    pub category: String,
    pub message: String,
}

impl TransportError {
    pub fn new(category: &str, message: String) -> Self {
        Self {
            category: category.to_string(),
            message,
        }
    }
}

pub trait SyncTransport: Send + Sync {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, TransportError>;
}
