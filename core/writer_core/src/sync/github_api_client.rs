//! GitHub Contents API 底层客户端 — 封装 HTTP 请求和错误分类。
//!
//! 此模块是 GitHub API 交互的最底层，负责：
//! - HTTP 请求构造和响应解析
//! - HTTP 状态码到业务错误分类的映射（`github_api_error`）
//! - SHA 冲突自动重试（`github_put_content_serial` / `github_delete_content_serial`）
//!
//! ## 错误分类契约
//!
//! `github_api_error` 返回的 `category` 字符串是跨平台契约——
//! Android/Linux/i18n 层根据 category 映射用户可见的错误提示。
//! 新增 category 必须同步更新所有平台的 i18n 资源。
//!
//! ## SHA 冲突重试
//!
//! GitHub Contents API 的 PUT/DELETE 要求提供文件的当前 SHA。
//! 并发写入时 SHA 可能过期（HTTP 409），`_serial` 函数会自动刷新 SHA 并重试一次。
//! 不做指数退避——同步是串行执行的，并发冲突由 LWW manifest 层面解决。

use base64::Engine;

/// 将 GitHub API HTTP 错误转换为带分类的 `crate::Error`。
///
/// `category` 分类规则（平台端 i18n 依赖此映射）：
/// - `token_invalid`：401，token 无效或过期
/// - `token_permission_denied`：403 + body 含 "Resource not accessible by personal access token"
/// - `auth_error`：403 但非权限拒绝（如 API rate limit 触发的 403 归入 `api_rate_limited`）
/// - `repo_not_found_or_no_permission`：404 且上下文为 get ref/tree/put/delete（仓库不存在或无权限）
/// - `file_not_found`：404 且上下文为 get contents（文件不存在，属于正常业务场景）
/// - `remote_sha_conflict`：409，并发写入导致 SHA 过期
/// - `api_rate_limited`：429，触发 GitHub API 速率限制
/// - `network_error`：5xx，服务端错误
/// - `api_error`：其他未分类错误
pub(crate) fn github_api_error(
    context: &str,
    status: reqwest::StatusCode,
    body: String,
) -> crate::Error {
    let status_u16 = status.as_u16();
    let category = match status_u16 {
        401 => "token_invalid",
        403 => {
            if body.contains("Resource not accessible by personal access token") {
                "token_permission_denied"
            } else {
                "auth_error"
            }
        }
        404 => {
            let ctx = context.to_lowercase();
            if ctx.contains("get ref") || ctx.contains("get recursive tree") {
                "repo_not_found_or_no_permission"
            } else if ctx.contains("get contents") {
                "file_not_found"
            } else {
                "repo_not_found_or_no_permission"
            }
        }
        409 => "remote_sha_conflict",
        429 => "api_rate_limited",
        _ => {
            if status.is_server_error() {
                "network_error"
            } else {
                "api_error"
            }
        }
    };
    let body_preview = body.chars().take(240).collect::<String>();
    crate::Error::SyncGithubApiError {
        category: category.to_string(),
        context: context.to_string(),
        status: status_u16,
        body_preview,
    }
}

/// 获取远程文件内容和 SHA。
///
/// 返回 `Some((bytes, sha))` 表示文件存在，`None` 表示 404（文件不存在，非错误）。
/// `bytes` 为文件内容的 base64 解码结果，`sha` 为 Git blob SHA（用于后续 PUT/DELETE 的冲突检测）。
pub(crate) fn github_get_content(
    client: &reqwest::blocking::Client,
    api_base: &str,
    token: &str,
    branch: &str,
    path: &str,
) -> crate::Result<Option<(Vec<u8>, Option<String>)>> {
    let url = format!("{}/contents/{}?ref={}", api_base, path, branch);
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "WriterApp/1.0")
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| crate::Error::SyncNetworkUnavailable { reason: e.to_string() })?;
    let status = resp.status();
    let body = resp
        .text()
        .map_err(|e| crate::Error::SyncNetworkUnavailable { reason: e.to_string() })?;
    if status.as_u16() == 404 {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(github_api_error(
            &format!("get contents {}", path),
            status,
            body,
        ));
    }
    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| crate::Error::SyncGithubApiError {
            category: "api_error".to_string(),
            context: format!("invalid contents json: {}", e),
            status: 0,
            body_preview: String::new(),
        })?;
    let sha = json["sha"].as_str().map(|s| s.to_string());
    let content_b64 = json["content"]
        .as_str()
        .unwrap_or_default()
        .replace('\n', "");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content_b64.as_bytes())
        .map_err(|e| {
            crate::Error::SyncGithubApiError {
                category: "api_error".to_string(),
                context: format!("invalid base64 for {}: {}", path, e),
                status: 0,
                body_preview: String::new(),
            }
        })?;
    Ok(Some((bytes, sha)))
}

/// 仅获取远程文件的 SHA，不下载内容。用于 DELETE 操作的前置查询。
pub(crate) fn github_get_content_sha(
    client: &reqwest::blocking::Client,
    api_base: &str,
    token: &str,
    branch: &str,
    path: &str,
) -> crate::Result<Option<String>> {
    Ok(github_get_content(client, api_base, token, branch, path)?.and_then(|(_, sha)| sha))
}

/// 上传或更新远程文件（单次尝试）。
///
/// `sha = Some(...)` 时为更新已有文件，`sha = None` 时为创建新文件。
/// 返回 HTTP 状态码和响应体，由调用方决定是否重试。
pub(crate) fn github_put_content_once(
    client: &reqwest::blocking::Client,
    api_base: &str,
    token: &str,
    branch: &str,
    path: &str,
    content: &[u8],
    sha: Option<&str>,
) -> crate::Result<(reqwest::StatusCode, String)> {
    let url = format!("{}/contents/{}", api_base, path);
    let mut payload = serde_json::json!({
        "message": format!("WriterApp sync {}", path),
        "content": base64::engine::general_purpose::STANDARD.encode(content),
        "branch": branch,
    });
    if let Some(sha) = sha {
        payload["sha"] = serde_json::json!(sha);
    }
    let resp = client
        .put(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "WriterApp/1.0")
        .header("Accept", "application/vnd.github+json")
        .json(&payload)
        .send()
        .map_err(|e| crate::Error::SyncNetworkUnavailable { reason: e.to_string() })?;
    let status = resp.status();
    let body = resp
        .text()
        .map_err(|e| crate::Error::SyncNetworkUnavailable { reason: e.to_string() })?;
    Ok((status, body))
}

/// 串行上传文件，自动处理 SHA 冲突（HTTP 409）。
///
/// 首次 PUT 失败且为 409 时，刷新远程 SHA 后重试一次。
/// 不做指数退避——同步流程串行执行，并发冲突由 LWW manifest 层面解决。
/// 重试仍失败则返回错误。
pub(crate) fn github_put_content_serial(
    client: &reqwest::blocking::Client,
    api_base: &str,
    token: &str,
    branch: &str,
    path: &str,
    content: &[u8],
    remote_sha: Option<String>,
) -> crate::Result<()> {
    let (status, body) = github_put_content_once(
        client,
        api_base,
        token,
        branch,
        path,
        content,
        remote_sha.as_deref(),
    )?;
    if status.is_success() {
        return Ok(());
    }
    if status.as_u16() == 409 {
        let refreshed_sha = github_get_content_sha(client, api_base, token, branch, path)?;
        let (retry_status, retry_body) = github_put_content_once(
            client,
            api_base,
            token,
            branch,
            path,
            content,
            refreshed_sha.as_deref(),
        )?;
        if retry_status.is_success() {
            return Ok(());
        }
        return Err(github_api_error(
            &format!("put contents {} after sha refresh", path),
            retry_status,
            retry_body,
        ));
    }
    Err(github_api_error(
        &format!("put contents {}", path),
        status,
        body,
    ))
}

/// 删除远程文件（单次尝试）。需要提供文件的当前 SHA。
///
/// 返回 HTTP 状态码和响应体。404 视为成功（文件已不存在）。
pub(crate) fn github_delete_content_once(
    client: &reqwest::blocking::Client,
    api_base: &str,
    token: &str,
    branch: &str,
    path: &str,
    sha: &str,
) -> crate::Result<(reqwest::StatusCode, String)> {
    let url = format!("{}/contents/{}", api_base, path);
    let payload = serde_json::json!({
        "message": format!("WriterApp delete {}", path),
        "sha": sha,
        "branch": branch,
    });
    let resp = client
        .delete(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "WriterApp/1.0")
        .header("Accept", "application/vnd.github+json")
        .json(&payload)
        .send()
        .map_err(|e| crate::Error::SyncNetworkUnavailable { reason: e.to_string() })?;
    let status = resp.status();
    let body = resp
        .text()
        .map_err(|e| crate::Error::SyncNetworkUnavailable { reason: e.to_string() })?;
    Ok((status, body))
}

/// 串行删除远程文件，自动处理 SHA 冲突（HTTP 409）。
///
/// 若 `remote_sha` 为 None 则跳过删除（文件在远程不存在）。
/// 首次 DELETE 失败且为 409 时，刷新远程 SHA 后重试一次。
/// 404 视为成功（文件已不存在），重试仍失败则返回错误。
pub(crate) fn github_delete_content_serial(
    client: &reqwest::blocking::Client,
    api_base: &str,
    token: &str,
    branch: &str,
    path: &str,
    remote_sha: Option<String>,
) -> crate::Result<()> {
    let Some(mut sha) = remote_sha else {
        return Ok(());
    };
    let (status, body) = github_delete_content_once(client, api_base, token, branch, path, &sha)?;
    if status.is_success() || status.as_u16() == 404 {
        return Ok(());
    }
    if status.as_u16() == 409 {
        if let Some(refreshed_sha) = github_get_content_sha(client, api_base, token, branch, path)?
        {
            sha = refreshed_sha;
            let (retry_status, retry_body) =
                github_delete_content_once(client, api_base, token, branch, path, &sha)?;
            if retry_status.is_success() || retry_status.as_u16() == 404 {
                return Ok(());
            }
            return Err(github_api_error(
                &format!("delete contents {} after sha refresh", path),
                retry_status,
                retry_body,
            ));
        }
        return Ok(());
    }
    Err(github_api_error(
        &format!("delete contents {}", path),
        status,
        body,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_api_error_404_get_ref_classified_as_repo_not_found() {
        let err = github_api_error(
            "get ref heads/main",
            reqwest::StatusCode::NOT_FOUND,
            "{}".to_string(),
        );
        assert_eq!(err.sync_category(), "repo_not_found_or_no_permission");
    }

    #[test]
    fn test_github_api_error_404_get_recursive_tree_classified_as_repo_not_found() {
        let err = github_api_error(
            "get recursive tree",
            reqwest::StatusCode::NOT_FOUND,
            "{}".to_string(),
        );
        assert_eq!(err.sync_category(), "repo_not_found_or_no_permission");
    }

    #[test]
    fn test_github_api_error_404_get_contents_classified_as_file_not_found() {
        let err = github_api_error(
            "get contents chapter.md",
            reqwest::StatusCode::NOT_FOUND,
            "{}".to_string(),
        );
        assert_eq!(err.sync_category(), "file_not_found");
    }

    #[test]
    fn test_github_api_error_404_put_contents_classified_as_repo_not_found() {
        let err = github_api_error(
            "put contents chapter.md",
            reqwest::StatusCode::NOT_FOUND,
            "{}".to_string(),
        );
        assert_eq!(err.sync_category(), "repo_not_found_or_no_permission");
    }

    #[test]
    fn test_github_api_error_404_delete_contents_classified_as_repo_not_found() {
        let err = github_api_error(
            "delete contents chapter.md",
            reqwest::StatusCode::NOT_FOUND,
            "{}".to_string(),
        );
        assert_eq!(err.sync_category(), "repo_not_found_or_no_permission");
    }

    #[test]
    fn test_github_api_error_401_classified_as_token_invalid() {
        let err = github_api_error(
            "get ref heads/main",
            reqwest::StatusCode::UNAUTHORIZED,
            "{}".to_string(),
        );
        assert_eq!(err.sync_category(), "token_invalid");
    }

    #[test]
    fn test_github_api_error_403_with_permission_denied_body() {
        let err = github_api_error(
            "get ref heads/main",
            reqwest::StatusCode::FORBIDDEN,
            "Resource not accessible by personal access token".to_string(),
        );
        assert_eq!(err.sync_category(), "token_permission_denied");
    }

    #[test]
    fn test_github_api_error_403_without_permission_denied_body() {
        let err = github_api_error(
            "get ref heads/main",
            reqwest::StatusCode::FORBIDDEN,
            "{}".to_string(),
        );
        assert_eq!(err.sync_category(), "auth_error");
    }

    #[test]
    fn test_github_api_error_404_generic_context_classified_as_repo_not_found() {
        let err = github_api_error(
            "some unknown operation",
            reqwest::StatusCode::NOT_FOUND,
            "{}".to_string(),
        );
        assert_eq!(err.sync_category(), "repo_not_found_or_no_permission");
    }

    #[test]
    fn test_github_api_error_404_not_found_category_not_used() {
        let contexts = [
            "get ref heads/main",
            "get recursive tree",
            "get contents chapter.md",
            "put contents chapter.md",
            "delete contents chapter.md",
            "some unknown operation",
        ];
        for ctx in &contexts {
            let err = github_api_error(ctx, reqwest::StatusCode::NOT_FOUND, "{}".to_string());
            let category = err.sync_category();
            assert!(
                category != "not_found",
                "404 for '{}' must not produce generic 'not_found' category, got: {}",
                ctx,
                category
            );
        }
    }
}
