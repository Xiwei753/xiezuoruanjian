use base64::Engine;

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
            } else if ctx.contains("put contents") || ctx.contains("delete contents") {
                "repo_not_found_or_no_permission"
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

pub(crate) fn github_get_content_sha(
    client: &reqwest::blocking::Client,
    api_base: &str,
    token: &str,
    branch: &str,
    path: &str,
) -> crate::Result<Option<String>> {
    Ok(github_get_content(client, api_base, token, branch, path)?.and_then(|(_, sha)| sha))
}

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
