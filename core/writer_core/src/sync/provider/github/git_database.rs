//! Git Database API 批量原子提交 — Issue #761 Part 2。
//!
//! 一次 `commit_batch` 的实现顺序固定为：
//! 1. GET 当前 branch ref（`GET /git/ref/heads/<branch>`）→ 拿到 head commit SHA；
//! 2. GET 当前 commit（`GET /git/commits/<sha>`）→ 拿到 tree SHA；
//! 3. 每个 Put 先 POST `/git/blobs`（`{"content":"<base64>","encoding":"base64"}`）
//!    → 拿到文件内容的 blob SHA；
//! 4. POST `/git/trees`，带 `base_tree`（第 2 步的 tree SHA）：
//!    - Put 用 tree entry 的 `sha`（第 3 步刚创建的 blob SHA）；
//!    - ReuseVersion 用 tree entry 的 `sha`（直接引用已有 blob SHA，不重新上传正文）；
//!    - Delete 用 `sha: null`（从 tree 中移除该路径）；
//! 5. POST `/git/commits`，parent=第 1 步读到的 head，tree=第 4 步返回的新 tree SHA；
//! 6. PATCH `/git/refs/heads/<branch>`，`sha`=第 5 步的新 commit SHA，`force=false`。
//!
//! Create a tree 的 `tree[].content` 是文件内容本身，不是 base64；base64 只在
//! Create a blob / Contents API 里出现。Put 一律先上传 blob 再用 `sha` 引用，
//! 保证 UTF-8 正文（以及未来可能出现的二进制内容）不被写坏。
//!
//! ref 更新失败（409/422）映射成 `ProviderError::PreconditionFailed`，回到现有 LWW/CAS
//! 重试，不允许 force 覆盖别人刚提交的 head。
//!
//! 参考官方接口：
//! - <https://docs.github.com/en/rest/git/blobs>
//! - <https://docs.github.com/en/rest/git/trees>
//! - <https://docs.github.com/en/rest/git/commits>
//! - <https://docs.github.com/en/rest/git/refs>

use std::collections::HashMap;

use writer_platform_api::SyncTransport;

use super::client::{
    get_commit, get_ref, get_tree_recursive, patch_ref, post_blob, post_commit, post_trees,
};
use super::error::map_http_error;
use crate::sync::provider::error::ProviderError;
use crate::sync::provider::model::{BatchCommitResult, BatchMutation, RemoteVersion};

/// 从 HTTP 响应 JSON 中提取字符串字段。
fn extract_str(json: &serde_json::Value, field: &str) -> Result<String, ProviderError> {
    json[field]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| ProviderError::Other {
            reason: format!("git_database: missing field `{field}` in response"),
        })
}

/// 从 `GET /git/ref/heads/<branch>` 响应中提取 head commit SHA。
///
/// 响应形如 `{object: {sha: "...", type: "commit"}, ref: "refs/heads/main"}`。
fn parse_ref_head_sha(body: &str) -> Result<String, ProviderError> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|e| ProviderError::Other {
        reason: format!("git_database: invalid ref json: {e}"),
    })?;
    extract_str(&json["object"], "sha")
}

/// 从 `GET /git/commits/<sha>` 响应中提取 tree SHA。
///
/// 响应形如 `{sha, tree: {sha: "...", type: "tree"}, parents: [...]}`。
fn parse_commit_tree_sha(body: &str) -> Result<String, ProviderError> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|e| ProviderError::Other {
        reason: format!("git_database: invalid commit json: {e}"),
    })?;
    extract_str(&json["tree"], "sha")
}

/// 从 `POST /git/trees` 响应中提取新 tree SHA。
fn parse_tree_sha(body: &str) -> Result<String, ProviderError> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|e| ProviderError::Other {
        reason: format!("git_database: invalid trees json: {e}"),
    })?;
    extract_str(&json, "sha")
}

/// 从 `POST /git/commits` 响应中提取新 commit SHA。
fn parse_commit_sha(body: &str) -> Result<String, ProviderError> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|e| ProviderError::Other {
        reason: format!("git_database: invalid commit create json: {e}"),
    })?;
    extract_str(&json, "sha")
}

/// 从 `POST /git/blobs` 响应中提取 blob SHA。
fn parse_blob_sha(body: &str) -> Result<String, ProviderError> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|e| ProviderError::Other {
        reason: format!("git_database: invalid blob json: {e}"),
    })?;
    extract_str(&json, "sha")
}

/// 上传单个 Put 的内容为 blob，返回 blob SHA（tree entry 引用用）。
fn create_blob(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    content: &[u8],
) -> Result<String, ProviderError> {
    let resp = post_blob(transport, api_base, token, content)?;
    let body = String::from_utf8(resp.body).unwrap_or_default();
    if !(200..300).contains(&resp.status) {
        return Err(map_http_error("git_database post_blob", resp.status, body));
    }
    parse_blob_sha(&body)
}

/// 把一组 [`BatchMutation`] 转成 Git tree entries（JSON 数组）。
///
/// - Put：内容先经 `POST /git/blobs` 上传，entry 用返回的 blob SHA；
/// - ReuseVersion：`{path, mode:"100644", type:"blob", sha:"<已有 blob sha>"}`；
/// - Delete：`{path, sha:null}`（GitHub 约定：sha=null 表示从 tree 移除）。
///
/// GitHub "Create a tree" 的 `tree[].content` 是文件内容本身而不是 base64，
/// 因此这里绝不用 `content` 承载 base64 文本；所有内容都用 `sha` 引用 blob。
fn mutations_to_tree_entries(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    mutations: &[BatchMutation],
) -> Result<Vec<serde_json::Value>, ProviderError> {
    let mut entries = Vec::with_capacity(mutations.len());
    for m in mutations {
        match m {
            BatchMutation::Put { path, content } => {
                let blob_sha = create_blob(transport, api_base, token, content)?;
                entries.push(serde_json::json!({
                    "path": path,
                    "mode": "100644",
                    "type": "blob",
                    "sha": blob_sha,
                }));
            }
            BatchMutation::ReuseVersion { path, version } => {
                entries.push(serde_json::json!({
                    "path": path,
                    "mode": "100644",
                    "type": "blob",
                    "sha": version.as_str(),
                }));
            }
            BatchMutation::Delete { path } => {
                entries.push(serde_json::json!({
                    "path": path,
                    "mode": "100644",
                    "type": "blob",
                    "sha": serde_json::Value::Null,
                }));
            }
            BatchMutation::DeleteIfMatch { path, .. } => {
                entries.push(serde_json::json!({
                    "path": path,
                    "mode": "100644",
                    "type": "blob",
                    "sha": serde_json::Value::Null,
                }));
            }
            BatchMutation::CheckObject { .. } | BatchMutation::CheckPrefix { .. } => {}
        }
    }
    Ok(entries)
}

/// 在 commit_batch 所基于的同一远端 tree 上校验条件 mutation。
/// 后续 ref 使用 force=false 更新；若校验后远端推进，ref 更新会失败。
fn validate_batch_preconditions(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    base_tree_sha: &str,
    mutations: &[BatchMutation],
) -> Result<(), ProviderError> {
    if !mutations.iter().any(|mutation| {
        matches!(
            mutation,
            BatchMutation::DeleteIfMatch { .. }
                | BatchMutation::CheckObject { .. }
                | BatchMutation::CheckPrefix { .. }
        )
    }) {
        return Ok(());
    }

    let response = get_tree_recursive(transport, api_base, token, base_tree_sha)?;
    let body = String::from_utf8(response.body).unwrap_or_default();
    if !(200..300).contains(&response.status) {
        return Err(map_http_error(
            "git_database get tree for batch preconditions",
            response.status,
            body,
        ));
    }
    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|error| ProviderError::Other {
            reason: format!("invalid tree JSON while validating batch preconditions: {error}"),
        })?;
    if json["truncated"].as_bool() == Some(true) {
        let path = mutations
            .iter()
            .find(|mutation| {
                matches!(
                    mutation,
                    BatchMutation::DeleteIfMatch { .. }
                        | BatchMutation::CheckObject { .. }
                        | BatchMutation::CheckPrefix { .. }
                )
            })
            .map(|mutation| mutation.path().to_string())
            .unwrap_or_else(|| "batch preconditions".to_string());
        return Err(ProviderError::PreconditionFailed {
            path,
            reason: "recursive tree was truncated; cannot verify batch preconditions".to_string(),
        });
    }
    let items = json["tree"]
        .as_array()
        .ok_or_else(|| ProviderError::Other {
            reason: "tree response missing 'tree' array while validating batch preconditions"
                .to_string(),
        })?;
    let mut versions = HashMap::with_capacity(items.len());
    for item in items {
        if item["type"].as_str() != Some("blob") {
            continue;
        }
        if let (Some(path), Some(version)) = (item["path"].as_str(), item["sha"].as_str()) {
            versions.insert(path.to_string(), version.to_string());
        }
    }

    for mutation in mutations {
        match mutation {
            BatchMutation::DeleteIfMatch { path, version } => {
                if versions.get(path).map(String::as_str) != Some(version.as_str()) {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.clone(),
                        reason: format!(
                            "version mismatch: expected={version}, current={:?}",
                            versions.get(path)
                        ),
                    });
                }
            }
            BatchMutation::CheckObject {
                path,
                expected_version,
            } => {
                let current = versions.get(path).map(String::as_str);
                if current != expected_version.as_ref().map(|version| version.as_str()) {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.clone(),
                        reason: format!(
                            "object version mismatch: expected={expected_version:?}, current={current:?}"
                        ),
                    });
                }
            }
            BatchMutation::CheckPrefix {
                prefix,
                expected_entries,
            } => {
                let prefix_with_slash = format!("{prefix}/");
                let mut current_entries: Vec<(String, String)> = versions
                    .iter()
                    .filter_map(|(path, version)| {
                        path.strip_prefix(&prefix_with_slash)
                            .map(|relative_path| (relative_path.to_string(), version.clone()))
                    })
                    .collect();
                current_entries.sort_by(|a, b| a.0.cmp(&b.0));
                let mut expected: Vec<(String, String)> = expected_entries
                    .iter()
                    .map(|entry| (entry.path.clone(), entry.version.0.clone()))
                    .collect();
                expected.sort_by(|a, b| a.0.cmp(&b.0));
                if current_entries != expected {
                    return Err(ProviderError::PreconditionFailed {
                        path: prefix.clone(),
                        reason: "directory object set or version changed".to_string(),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// 执行 Git Database API 原子批量提交。
///
/// 调用方为 `GitHubProvider::commit_batch`，传入已构造好的 mutations 和 commit message。
/// 任一步 HTTP 错误向上返回 `ProviderError`；ref PATCH 返回 409/422 时映射成
/// `PreconditionFailed`，让 LWW/CAS 重试。
///
/// 成功返回 [`BatchCommitResult`]，`revision` 为新 commit SHA，`touched_paths`
/// 为本次 batch 实际生效的路径（来自 mutations 的 path）。
pub(crate) fn commit_batch_via_git_database(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    mutations: &[BatchMutation],
    message: &str,
) -> Result<BatchCommitResult, ProviderError> {
    commit_batch_via_git_database_at(
        transport, api_base, token, branch, None, mutations, message,
    )
}

/// 带远端基线校验的 Git Database 提交。
///
/// `expected_head` 是整轮规划使用的 branch HEAD。提交前重新 GET ref 后必须
/// 仍等于该值；否则其他设备已更新远端，旧 LWW 结果不能提交到新 base。
pub(crate) fn commit_batch_via_git_database_at(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    expected_head: Option<&str>,
    mutations: &[BatchMutation],
    message: &str,
) -> Result<BatchCommitResult, ProviderError> {
    // 1. GET /git/ref/heads/<branch> → head commit SHA
    let ref_resp = get_ref(transport, api_base, token, branch)?;
    let ref_body = String::from_utf8(ref_resp.body).unwrap_or_default();
    if !(200..300).contains(&ref_resp.status) {
        return Err(map_http_error(
            "git_database get_ref",
            ref_resp.status,
            ref_body,
        ));
    }
    let head_sha = parse_ref_head_sha(&ref_body)?;
    if let Some(expected_head) = expected_head {
        if expected_head != head_sha {
            return Err(ProviderError::PreconditionFailed {
                path: format!("refs/heads/{branch}"),
                reason: "remote branch head changed since the sync plan was built".to_string(),
            });
        }
    }

    // 2. GET /git/commits/<head_sha> → tree SHA
    let commit_resp = get_commit(transport, api_base, token, &head_sha)?;
    let commit_body = String::from_utf8(commit_resp.body).unwrap_or_default();
    if !(200..300).contains(&commit_resp.status) {
        return Err(map_http_error(
            "git_database get_commit",
            commit_resp.status,
            commit_body,
        ));
    }
    let base_tree_sha = parse_commit_tree_sha(&commit_body)?;

    validate_batch_preconditions(transport, api_base, token, &base_tree_sha, mutations)?;

    // 3. 每个 Put 先 POST /git/blobs 拿 blob SHA，再构造 tree entries。
    let tree_entries = mutations_to_tree_entries(transport, api_base, token, mutations)?;

    // 4. POST /git/trees（base_tree + tree entries）→ 新 tree SHA
    let trees_resp = post_trees(transport, api_base, token, &base_tree_sha, &tree_entries)?;
    let trees_body = String::from_utf8(trees_resp.body).unwrap_or_default();
    if !(200..300).contains(&trees_resp.status) {
        return Err(map_http_error(
            "git_database post_trees",
            trees_resp.status,
            trees_body,
        ));
    }
    let new_tree_sha = parse_tree_sha(&trees_body)?;

    // No-change rounds must not create an empty commit or PATCH the ref.  The
    // GitHub tree API returns the same SHA when the complete tree is unchanged.
    if new_tree_sha == base_tree_sha {
        return Ok(BatchCommitResult {
            revision: RemoteVersion(head_sha),
            touched_paths: Vec::new(),
        });
    }

    // 4. POST /git/commits（parent=head, tree=新 tree）→ 新 commit SHA
    let new_commit_resp = post_commit(
        transport,
        api_base,
        token,
        message,
        &new_tree_sha,
        &head_sha,
    )?;
    let new_commit_body = String::from_utf8(new_commit_resp.body).unwrap_or_default();
    if !(200..300).contains(&new_commit_resp.status) {
        return Err(map_http_error(
            "git_database post_commit",
            new_commit_resp.status,
            new_commit_body,
        ));
    }
    let new_commit_sha = parse_commit_sha(&new_commit_body)?;

    // 5. PATCH /git/refs/heads/<branch>（sha=新 commit, force=false）
    let patch_resp = patch_ref(transport, api_base, token, branch, &new_commit_sha)?;
    let patch_body = String::from_utf8(patch_resp.body).unwrap_or_default();
    if !(200..300).contains(&patch_resp.status) {
        // 409/422 → PreconditionFailed（ref 已被别人推进，回到 LWW/CAS 重试）。
        // GitHub "Update a reference" 对非 fast-forward 更新返回 409 或 422
        // （<https://docs.github.com/en/rest/git/refs>），两者都按乐观并发冲突处理；
        // `force=false` 保证绝不覆盖别人刚提交的 head。
        if matches!(patch_resp.status, 409 | 422) {
            return Err(ProviderError::PreconditionFailed {
                path: format!("refs/heads/{branch}"),
                reason: format!(
                    "git_database patch_ref {}: remote ref moved; {}",
                    patch_resp.status,
                    patch_body.chars().take(200).collect::<String>()
                ),
            });
        }
        return Err(map_http_error(
            "git_database patch_ref",
            patch_resp.status,
            patch_body,
        ));
    }

    let touched_paths = mutations
        .iter()
        .filter(|mutation| {
            !matches!(
                mutation,
                BatchMutation::CheckObject { .. } | BatchMutation::CheckPrefix { .. }
            )
        })
        .map(|mutation| mutation.path().to_string())
        .collect();
    Ok(BatchCommitResult {
        revision: RemoteVersion(new_commit_sha),
        touched_paths,
    })
}

#[cfg(test)]
mod tests;
