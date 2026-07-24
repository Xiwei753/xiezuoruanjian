//! 默认同步传输实现 — 基于 reqwest 的 HTTP 客户端。
//!
//! 当平台端通过 UniFFI 的 `open_workspace_with_init` 创建服务时，
//! 无法注入 `SyncTransportFactory`（UniFFI 不支持传递 trait 对象）。
//! 此模块提供基于 reqwest 的默认实现，确保通过 UniFFI 创建的服务也有同步传输能力。
//!
//! 平台 crate 可以通过 `PlatformServices::sync_transport_factory` 注入自定义实现，
//! 覆盖此默认行为。

#[cfg(feature = "github-api")]
use writer_platform_api::{HttpRequest, HttpResponse, SyncTransport, TransportError};

#[cfg(feature = "github-api")]
pub(crate) struct DefaultSyncTransport {
    client: reqwest::blocking::Client,
}

#[cfg(feature = "github-api")]
impl DefaultSyncTransport {
    pub fn new() -> Result<Self, TransportError> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("WriterApp/1.0")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| TransportError::new("init", format!("Failed to build HTTP client: {}", e)))?;
        Ok(Self { client })
    }

    pub fn factory() -> writer_platform_api::SyncTransportFactory {
        std::sync::Arc::new(|| -> Box<dyn SyncTransport> {
            let transport = Self::new()
                .unwrap_or_else(|e| panic!("Failed to create default SyncTransport: {}", e.message));
            Box::new(transport)
        })
    }
}

#[cfg(feature = "github-api")]
impl SyncTransport for DefaultSyncTransport {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        let mut req = match request.method.as_str() {
            "GET" => self.client.get(&request.url),
            "PUT" => self.client.put(&request.url),
            "DELETE" => self.client.delete(&request.url),
            "POST" => self.client.post(&request.url),
            "PATCH" => self.client.patch(&request.url),
            "HEAD" => self.client.head(&request.url),
            _ => {
                return Err(TransportError::new(
                    "invalid_method",
                    format!("Unsupported HTTP method: {}", request.method),
                ));
            }
        };

        for (key, value) in &request.headers {
            req = req.header(key.as_str(), value.as_str());
        }

        if let Some(body) = request.body {
            req = req.body(body);
        }

        let resp = req.send().map_err(|e| {
            if e.is_connect() {
                TransportError::new("dns_failed", e.to_string())
            } else if e.is_timeout() {
                TransportError::new("timeout", e.to_string())
            } else {
                TransportError::new("network", e.to_string())
            }
        })?;

        let status = resp.status().as_u16();
        let headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let body = resp.bytes().map_err(|e| {
            TransportError::new("response_read", e.to_string())
        })?;
        Ok(HttpResponse {
            status,
            headers,
            body: body.to_vec(),
        })
    }
}
