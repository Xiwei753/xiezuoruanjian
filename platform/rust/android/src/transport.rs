//! Android 同步 HTTP 传输实现。
//!
//! 基于 `reqwest` 阻塞客户端提供 `SyncTransport` 实现，仅在 `github-api`
//! feature 启用时编译。

use writer_platform_api::{HttpRequest, HttpResponse, SyncTransport, TransportError};

#[cfg(feature = "github-api")]
pub struct ReqwestSyncTransport {
    client: reqwest::blocking::Client,
}

#[cfg(feature = "github-api")]
impl ReqwestSyncTransport {
    pub fn new() -> Result<Self, TransportError> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("WriterApp/1.0")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| {
                TransportError::new("init", format!("Failed to build HTTP client: {}", e))
            })?;
        Ok(Self { client })
    }
}

#[cfg(feature = "github-api")]
impl SyncTransport for ReqwestSyncTransport {
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
        let body = resp
            .bytes()
            .map_err(|e| TransportError::new("response_read", e.to_string()))?;
        Ok(HttpResponse {
            status,
            headers,
            body: body.to_vec(),
        })
    }
}

#[cfg(feature = "github-api")]
pub fn create_sync_transport() -> Result<Box<dyn SyncTransport>, TransportError> {
    let transport = ReqwestSyncTransport::new()?;
    Ok(Box::new(transport))
}
