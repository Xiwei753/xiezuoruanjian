//! GitHub REST 客户端 — 迁移自 `github_api_client.rs`，返回 [`ProviderError`]。
//!
//! 本模块是 GitHub API 交互的最底层，负责：
//! - HTTP 请求构造和响应解析
//! - HTTP 状态码到 [`ProviderError`] 的映射（`super::error::map_http_error`）
//! - SHA 冲突自动重试（`put_content_serial` / `delete_content_serial`）
//!
//! 与旧 `github_api_client.rs` 的区别：返回 `ProviderError` 而非 `crate::Error`，
//! 让通用层不依赖 GitHub 特定错误名。调用方通过 `From<ProviderError> for crate::Error` 转换。

use base64::Engine;
use writer_platform_api::{HttpRequest, HttpResponse, SyncTransport, TransportError};

use super::error::map_http_error;
use crate::sync::provider::error::ProviderError;

fn transport_err_to_provider(e: TransportError) -> ProviderError {
    ProviderError::Network {
        reason: format!("{}: {}", e.category, e.message),
    }
}

pub(crate) fn execute_get(
    transport: &dyn SyncTransport,
    url: &str,
    token: &str,
) -> Result<HttpResponse, ProviderError> {
    let request = HttpRequest {
        method: "GET".to_string(),
        url: url.to_string(),
        headers: vec![
            ("Authorization".to_string(), format!("Bearer {}", token)),
            ("User-Agent".to_string(), "WriterApp/1.0".to_string()),
            (
                "Accept".to_string(),
                "application/vnd.github+json".to_string(),
            ),
        ],
        body: None,
    };
    transport
        .execute(request)
        .map_err(transport_err_to_provider)
}

pub(crate) fn execute_json(
    transport: &dyn SyncTransport,
    method: &str,
    url: &str,
    token: &str,
    payload: &serde_json::Value,
) -> Result<HttpResponse, ProviderError> {
    let body_bytes = serde_json::to_vec(payload).map_err(|e| ProviderError::Other {
        reason: format!("json serialize: {}", e),
    })?;
    let request = HttpRequest {
        method: method.to_string(),
        url: url.to_string(),
        headers: vec![
            ("Authorization".to_string(), format!("Bearer {}", token)),
            ("User-Agent".to_string(), "WriterApp/1.0".to_string()),
            (
                "Accept".to_string(),
                "application/vnd.github+json".to_string(),
            ),
            ("Content-Type".to_string(), "application/json".to_string()),
        ],
        body: Some(body_bytes),
    };
    transport
        .execute(request)
        .map_err(transport_err_to_provider)
}

fn is_success_status(status: u16) -> bool {
    (200..300).contains(&status)
}

/// GitHub 文件内容：字节 + 可选 SHA（blob hash）。
pub(crate) type GitHubContent = Option<(Vec<u8>, Option<String>)>;

/// 获取远程文件内容和 SHA。
///
/// 返回 `Some((bytes, sha))` 表示文件存在，`None` 表示 404（文件不存在，非错误）。
#[allow(clippy::too_many_lines)]
pub(crate) fn get_content(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    path: &str,
) -> Result<GitHubContent, ProviderError> {
    let url = format!("{}/contents/{}?ref={}", api_base, path, branch);
    let resp = execute_get(transport, &url, token)?;
    let status = resp.status;
    let body = String::from_utf8(resp.body).unwrap_or_default();
    if status == 404 {
        return Ok(None);
    }
    if !is_success_status(status) {
        return Err(map_http_error(
            &format!("get contents {}", path),
            status,
            body,
        ));
    }
    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| ProviderError::Other {
            reason: format!("invalid contents json for {}: {}", path, e),
        })?;
    let sha = json["sha"].as_str().map(|s| s.to_string());
    let content_b64 = json["content"]
        .as_str()
        .unwrap_or_default()
        .replace('\n', "");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content_b64.as_bytes())
        .map_err(|e| ProviderError::Other {
            reason: format!("invalid base64 for {}: {}", path, e),
        })?;
    Ok(Some((bytes, sha)))
}

/// 仅获取远程文件的 SHA，不下载内容。用于 DELETE 操作的前置查询。
pub(crate) fn get_content_sha(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    path: &str,
) -> Result<Option<String>, ProviderError> {
    Ok(get_content(transport, api_base, token, branch, path)?.and_then(|(_, sha)| sha))
}

/// 上传或更新远程文件（单次尝试）。
///
/// `sha = Some(...)` 时为更新已有文件，`sha = None` 时为创建新文件。
/// 返回 HTTP 状态码和响应体，由调用方决定是否重试。
pub(crate) fn put_content_once(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    path: &str,
    content: &[u8],
    sha: Option<&str>,
) -> Result<(u16, String), ProviderError> {
    let url = format!("{}/contents/{}", api_base, path);
    let mut payload = serde_json::json!({
        "message": format!("WriterApp sync {}", path),
        "content": base64::engine::general_purpose::STANDARD.encode(content),
        "branch": branch,
    });
    if let Some(sha) = sha {
        payload["sha"] = serde_json::json!(sha);
    }
    let resp = execute_json(transport, "PUT", &url, token, &payload)?;
    let body = String::from_utf8(resp.body).unwrap_or_default();
    Ok((resp.status, body))
}

/// 串行上传文件，自动处理 SHA 冲突（HTTP 409）。
///
/// 首次 PUT 失败且为 409 时，刷新远程 SHA 后重试一次。
/// 重试仍失败则返回错误。
pub(crate) fn put_content_serial(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    path: &str,
    content: &[u8],
    remote_sha: Option<String>,
) -> Result<(), ProviderError> {
    let (status, body) = put_content_once(
        transport,
        api_base,
        token,
        branch,
        path,
        content,
        remote_sha.as_deref(),
    )?;
    if is_success_status(status) {
        return Ok(());
    }
    if status == 409 {
        let refreshed_sha = get_content_sha(transport, api_base, token, branch, path)?;
        let (retry_status, retry_body) = put_content_once(
            transport,
            api_base,
            token,
            branch,
            path,
            content,
            refreshed_sha.as_deref(),
        )?;
        if is_success_status(retry_status) {
            return Ok(());
        }
        return Err(map_http_error(
            &format!("put contents {} after sha refresh", path),
            retry_status,
            retry_body,
        ));
    }
    Err(map_http_error(
        &format!("put contents {}", path),
        status,
        body,
    ))
}

/// 删除远程文件（单次尝试）。需要提供文件的当前 SHA。
///
/// 返回 HTTP 状态码和响应体。404 视为成功（文件已不存在）。
pub(crate) fn delete_content_once(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    path: &str,
    sha: &str,
) -> Result<(u16, String), ProviderError> {
    let url = format!("{}/contents/{}", api_base, path);
    let payload = serde_json::json!({
        "message": format!("WriterApp delete {}", path),
        "sha": sha,
        "branch": branch,
    });
    let resp = execute_json(transport, "DELETE", &url, token, &payload)?;
    let body = String::from_utf8(resp.body).unwrap_or_default();
    Ok((resp.status, body))
}

/// 串行删除远程文件，自动处理 SHA 冲突（HTTP 409）。
///
/// 若 `remote_sha` 为 None 则跳过删除（文件在远程不存在）。
/// 首次 DELETE 失败且为 409 时，刷新远程 SHA 后重试一次。
/// 404 视为成功（文件已不存在），重试仍失败则返回错误。
pub(crate) fn delete_content_serial(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    path: &str,
    remote_sha: Option<String>,
) -> Result<(), ProviderError> {
    let Some(mut sha) = remote_sha else {
        return Ok(());
    };
    let (status, body) = delete_content_once(transport, api_base, token, branch, path, &sha)?;
    if is_success_status(status) || status == 404 {
        return Ok(());
    }
    if status == 409 {
        if let Some(refreshed_sha) = get_content_sha(transport, api_base, token, branch, path)? {
            sha = refreshed_sha;
            let (retry_status, retry_body) =
                delete_content_once(transport, api_base, token, branch, path, &sha)?;
            if is_success_status(retry_status) || retry_status == 404 {
                return Ok(());
            }
            return Err(map_http_error(
                &format!("delete contents {} after sha refresh", path),
                retry_status,
                retry_body,
            ));
        }
        return Ok(());
    }
    Err(map_http_error(
        &format!("delete contents {}", path),
        status,
        body,
    ))
}

/// 查询远端 Git tree（recursive），返回原始 HTTP 响应。
///
/// 供 `GitHubProvider::list` 解析 tree 结构。404 由调用方处理（区分空仓库/分支不存在）。
pub(crate) fn get_tree_recursive(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
) -> Result<HttpResponse, ProviderError> {
    let url = format!("{}/git/trees/{}?recursive=1", api_base, branch);
    execute_get(transport, &url, token)
}

/// 查询远端 ref 是否存在（用于 list 的 404 诊断：区分空仓库 vs 分支不存在）。
pub(crate) fn get_ref(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
) -> Result<HttpResponse, ProviderError> {
    let url = format!("{}/git/ref/heads/{}", api_base, branch);
    execute_get(transport, &url, token)
}

/// 查询仓库根（用于 list 的 404 诊断：区分仓库不存在 vs 权限不足）。
pub(crate) fn get_repo(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
) -> Result<HttpResponse, ProviderError> {
    // api_base 形如 https://api.github.com/repos/owner/repo，repo 根即 api_base 本身。
    execute_get(transport, api_base, token)
}
