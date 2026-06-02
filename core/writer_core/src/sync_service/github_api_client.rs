use base64::Engine;

pub(crate) fn github_api_error(
    context: &str,
    status: reqwest::StatusCode,
    body: String,
) -> crate::Error {
    let status_u16 = status.as_u16();
    let category = match status_u16 {
        401 | 403 => "auth_error",
        404 => "not_found",
        409 => "remote_sha_conflict",
        429 => "api_rate_limited",
        _ => {
            let lower = body.to_lowercase();
            if lower.contains("rate limit") {
                "api_rate_limited"
            } else if status.is_server_error() {
                "network_error"
            } else {
                "api_error"
            }
        }
    };
    let body_preview = body.chars().take(240).collect::<String>();
    crate::Error::Other(format!(
        "{}: {} failed with HTTP {}: {}",
        category, context, status_u16, body_preview
    ))
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
        .map_err(|e| crate::Error::Other(format!("network_error: {}", e)))?;
    let status = resp.status();
    let body = resp
        .text()
        .map_err(|e| crate::Error::Other(format!("network_error: {}", e)))?;
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
        .map_err(|e| crate::Error::Other(format!("api_error: invalid contents json: {}", e)))?;
    let sha = json["sha"].as_str().map(|s| s.to_string());
    let content_b64 = json["content"]
        .as_str()
        .unwrap_or_default()
        .replace('\n', "");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content_b64.as_bytes())
        .map_err(|e| {
            crate::Error::Other(format!("api_error: invalid base64 for {}: {}", path, e))
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
        .map_err(|e| crate::Error::Other(format!("network_error: {}", e)))?;
    let status = resp.status();
    let body = resp
        .text()
        .map_err(|e| crate::Error::Other(format!("network_error: {}", e)))?;
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
        .map_err(|e| crate::Error::Other(format!("network_error: {}", e)))?;
    let status = resp.status();
    let body = resp
        .text()
        .map_err(|e| crate::Error::Other(format!("network_error: {}", e)))?;
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
