//! Git Database API 批量原子提交 — Issue #761 Part 2。
//!
//! 一次 `commit_batch` 的实现顺序固定为：
//! 1. GET 当前 branch ref（`GET /git/ref/heads/<branch>`）→ 拿到 head commit SHA；
//! 2. GET 当前 commit（`GET /git/commits/<sha>`）→ 拿到 tree SHA；
//! 3. POST `/git/trees`，带 `base_tree`（第 2 步的 tree SHA）：
//!    - Put 用 tree entry 的 `content`（base64 编码的 UTF-8 内容，mode="100644", type="blob"）；
//!    - ReuseVersion 用 tree entry 的 `sha`（直接引用已有 blob SHA，不重新上传正文）；
//!    - Delete 用 `sha: null`（从 tree 中移除该路径）；
//! 4. POST `/git/commits`，parent=第 1 步读到的 head，tree=第 3 步返回的新 tree SHA；
//! 5. PATCH `/git/refs/heads/<branch>`，`sha`=第 4 步的新 commit SHA，`force=false`。
//!
//! ref 更新失败（409/422）映射成 `ProviderError::PreconditionFailed`，回到现有 LWW/CAS
//! 重试，不允许 force 覆盖别人刚提交的 head。
//!
//! 参考官方接口：
//! - <https://docs.github.com/en/rest/git/trees>
//! - <https://docs.github.com/en/rest/git/commits>
//! - <https://docs.github.com/en/rest/git/refs>

use base64::Engine;
use writer_platform_api::SyncTransport;

use super::client::{get_commit, get_ref, patch_ref, post_commit, post_trees};
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

/// 把一组 [`BatchMutation`] 转成 Git tree entries（JSON 数组）。
///
/// - Put：`{path, mode:"100644", type:"blob", content:"<base64 UTF-8>"}`
/// - ReuseVersion：`{path, mode:"100644", type:"blob", sha:"<blob sha>"}`
/// - Delete：`{path, sha:null}`（GitHub 约定：sha=null 表示从 tree 移除）
fn mutations_to_tree_entries(mutations: &[BatchMutation]) -> Vec<serde_json::Value> {
    let mut entries = Vec::with_capacity(mutations.len());
    for m in mutations {
        match m {
            BatchMutation::Put { path, content } => {
                let content_b64 = base64::engine::general_purpose::STANDARD.encode(content);
                entries.push(serde_json::json!({
                    "path": path,
                    "mode": "100644",
                    "type": "blob",
                    "content": content_b64,
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
        }
    }
    entries
}

/// 执行 Git Database API 5 步原子批量提交。
///
/// 调用方为 `GitHubProvider::commit_batch`，传入已构造好的 mutations 和 commit message。
/// 任一步 HTTP 错误向上返回 `ProviderError`；ref PATCH 返回 409 时映射成
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

    // 3. POST /git/trees（base_tree + tree entries）→ 新 tree SHA
    let tree_entries = mutations_to_tree_entries(mutations);
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

    let touched_paths = mutations.iter().map(|m| m.path().to_string()).collect();
    Ok(BatchCommitResult {
        revision: RemoteVersion(new_commit_sha),
        touched_paths,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use writer_platform_api::{HttpRequest, HttpResponse, SyncTransport, TransportError};

    use super::*;

    /// 记录请求、按顺序返回预设响应的假 transport。
    ///
    /// 用于断言 Git Database API 的请求顺序和 payload 形状，
    /// 不依赖真实网络。
    struct CannedTransport {
        responses: Mutex<VecDeque<HttpResponse>>,
        requests: Mutex<Vec<HttpRequest>>,
    }

    impl CannedTransport {
        fn new(responses: Vec<HttpResponse>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn requests(&self) -> Vec<HttpRequest> {
            self.requests
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }
    }

    impl SyncTransport for CannedTransport {
        fn execute(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
            self.requests
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(request);
            self.responses
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pop_front()
                .ok_or_else(|| {
                    TransportError::new("test", "canned responses exhausted".to_string())
                })
        }
    }

    fn json_response(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    /// 读取请求的 JSON body。
    fn request_json(req: &HttpRequest) -> serde_json::Value {
        serde_json::from_slice(req.body.as_ref().expect("request body")).expect("request json")
    }

    /// 5 步成功响应：ref → commit → trees → commits → ref PATCH。
    fn success_responses() -> Vec<HttpResponse> {
        vec![
            json_response(
                200,
                r#"{"ref":"refs/heads/main","object":{"sha":"head_c","type":"commit"}}"#,
            ),
            json_response(
                200,
                r#"{"sha":"head_c","tree":{"sha":"base_t","type":"tree"},"parents":[]}"#,
            ),
            json_response(201, r#"{"sha":"new_t","tree":[]}"#),
            json_response(
                201,
                r#"{"sha":"new_c","tree":{"sha":"new_t"},"parents":[{"sha":"head_c"}]}"#,
            ),
            json_response(
                200,
                r#"{"ref":"refs/heads/main","object":{"sha":"new_c","type":"commit"}}"#,
            ),
        ]
    }

    fn sample_mutations() -> Vec<BatchMutation> {
        vec![
            BatchMutation::Put {
                path: "projects/p1/__generations__/g1/a.md".to_string(),
                content: b"hello".to_vec(),
            },
            BatchMutation::ReuseVersion {
                path: "projects/p1/__generations__/g1/b.md".to_string(),
                version: RemoteVersion("blob_sha_b".to_string()),
            },
            BatchMutation::Delete {
                path: "projects/p1/__generations__/g1/c.md".to_string(),
            },
        ]
    }

    const API_BASE: &str = "https://api.github.com/repos/owner/repo";

    /// Issue #761 Part 2：5 步 Git Database API 调用顺序与 payload 形状。
    ///
    /// 一次 batch 必须恰好产生 5 个请求，且不触碰 Contents API：
    /// 1. GET ref → head commit；
    /// 2. GET commit → base tree；
    /// 3. POST trees（base_tree + content/sha/null 三种 entry）；
    /// 4. POST commits（parent=head，tree=新 tree）；
    /// 5. PATCH ref（force=false）。
    #[test]
    fn commit_batch_runs_five_git_database_steps_in_order() {
        let transport = CannedTransport::new(success_responses());
        let mutations = sample_mutations();

        let result = commit_batch_via_git_database(
            &transport,
            API_BASE,
            "tok",
            "main",
            &mutations,
            "WriterApp publish generation g1",
        )
        .expect("batch should succeed");

        assert_eq!(result.revision.as_str(), "new_c");
        assert_eq!(
            result.touched_paths,
            vec![
                "projects/p1/__generations__/g1/a.md".to_string(),
                "projects/p1/__generations__/g1/b.md".to_string(),
                "projects/p1/__generations__/g1/c.md".to_string(),
            ]
        );

        let requests = transport.requests();
        assert_eq!(requests.len(), 5, "一次 batch 恰好 5 个请求");
        let seen: Vec<(&str, String)> = requests
            .iter()
            .map(|r| (r.method.as_str(), r.url.clone()))
            .collect();
        assert_eq!(
            seen,
            vec![
                ("GET", format!("{API_BASE}/git/ref/heads/main")),
                ("GET", format!("{API_BASE}/git/commits/head_c")),
                ("POST", format!("{API_BASE}/git/trees")),
                ("POST", format!("{API_BASE}/git/commits")),
                ("PATCH", format!("{API_BASE}/git/refs/heads/main")),
            ]
        );
        assert!(
            !requests.iter().any(|r| r.url.contains("/contents/")),
            "generation 发布不得触碰 Contents API 单文件入口"
        );
        assert!(
            requests.iter().all(|r| r
                .headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "Bearer tok")),
            "每个请求都应带 Bearer token"
        );

        // POST /git/trees：base_tree + 三种 entry（Put 用 content，ReuseVersion 用 sha，Delete 用 sha:null）。
        let trees = request_json(&requests[2]);
        assert_eq!(trees["base_tree"], "base_t");
        let entries = trees["tree"].as_array().expect("tree entries");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0]["path"], "projects/p1/__generations__/g1/a.md");
        assert_eq!(entries[0]["mode"], "100644");
        assert_eq!(entries[0]["type"], "blob");
        // base64("hello")
        assert_eq!(entries[0]["content"], "aGVsbG8=");
        assert_eq!(entries[1]["sha"], "blob_sha_b");
        assert!(entries[1].get("content").is_none());
        assert_eq!(entries[2]["sha"], serde_json::Value::Null);

        // POST /git/commits：新 tree + parent=head。
        let commit = request_json(&requests[3]);
        assert_eq!(commit["message"], "WriterApp publish generation g1");
        assert_eq!(commit["tree"], "new_t");
        assert_eq!(commit["parents"][0], "head_c");

        // PATCH /git/refs：新 commit + force=false（绝不覆盖别人刚提交的 head）。
        let patch = request_json(&requests[4]);
        assert_eq!(patch["sha"], "new_c");
        assert_eq!(patch["force"], false);
    }

    /// Issue #761 Part 2：ref 更新冲突（409/422）映射成 `PreconditionFailed`。
    #[test]
    fn commit_batch_maps_ref_conflict_to_precondition_failed() {
        for status in [409_u16, 422_u16] {
            let mut responses = success_responses();
            responses[4] = json_response(status, r#"{"message":"Update is not a fast forward"}"#);
            let transport = CannedTransport::new(responses);

            let err = commit_batch_via_git_database(
                &transport,
                API_BASE,
                "tok",
                "main",
                &sample_mutations(),
                "publish",
            )
            .expect_err("ref update rejection must fail the batch");

            match err {
                ProviderError::PreconditionFailed { path, reason } => {
                    assert_eq!(path, "refs/heads/main");
                    assert!(
                        reason.contains(&status.to_string()),
                        "reason 应带 HTTP {status} 上下文，实际 {reason}"
                    );
                }
                other => panic!("{status} 必须映射成 PreconditionFailed，实际 {other:?}"),
            }
            assert_eq!(
                transport.requests().len(),
                5,
                "ref 冲突发生在第 5 步，前 4 步已执行"
            );
        }
    }

    /// Issue #761 Part 2：tree 创建失败时不创建 commit、不推进 ref。
    #[test]
    fn commit_batch_stops_before_commit_when_tree_creation_fails() {
        let mut responses = success_responses();
        responses[2] = json_response(422, r#"{"message":"Invalid tree entry"}"#);
        let transport = CannedTransport::new(responses);

        let err = commit_batch_via_git_database(
            &transport,
            API_BASE,
            "tok",
            "main",
            &sample_mutations(),
            "publish",
        )
        .expect_err("tree creation failure must fail the batch");

        assert!(
            !matches!(err, ProviderError::PreconditionFailed { .. }),
            "tree 创建失败不是 CAS 冲突，实际 {err:?}"
        );
        let requests = transport.requests();
        assert_eq!(
            requests.len(),
            3,
            "tree 创建失败后不得再 POST commit / PATCH ref，实际 {} 个请求",
            requests.len()
        );
        assert!(
            !requests
                .iter()
                .any(|r| r.url.ends_with("/git/commits") || r.url.contains("/git/refs/")),
            "tree 创建失败后不得推进 commit / ref"
        );
    }

    #[test]
    fn parse_ref_head_sha_extracts_object_sha() {
        let body = r#"{"ref":"refs/heads/main","node_id":"R","url":"u","object":{"sha":"abc","type":"commit","url":"u2"}}"#;
        assert_eq!(parse_ref_head_sha(body).unwrap(), "abc");
    }

    #[test]
    fn parse_commit_tree_sha_extracts_tree_sha() {
        let body = r#"{"sha":"c","tree":{"sha":"t","type":"tree","url":"u"},"parents":[]}"#;
        assert_eq!(parse_commit_tree_sha(body).unwrap(), "t");
    }

    #[test]
    fn parse_tree_sha_extracts_top_level_sha() {
        let body = r#"{"sha":"treenew","tree":[]}"#;
        assert_eq!(parse_tree_sha(body).unwrap(), "treenew");
    }

    #[test]
    fn parse_commit_sha_extracts_top_level_sha() {
        let body = r#"{"sha":"commitnew","tree":{"sha":"t"},"parents":[]}"#;
        assert_eq!(parse_commit_sha(body).unwrap(), "commitnew");
    }

    #[test]
    fn mutations_to_tree_entries_put_uses_base64_content() {
        let mutations = vec![BatchMutation::Put {
            path: "a/b.txt".to_string(),
            content: b"hello".to_vec(),
        }];
        let entries = mutations_to_tree_entries(&mutations);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["path"], "a/b.txt");
        assert_eq!(entries[0]["mode"], "100644");
        assert_eq!(entries[0]["type"], "blob");
        // base64("hello") = "aGVsbG8="
        assert_eq!(entries[0]["content"], "aGVsbG8=");
    }

    #[test]
    fn mutations_to_tree_entries_reuse_version_uses_sha() {
        let mutations = vec![BatchMutation::ReuseVersion {
            path: "a/c.txt".to_string(),
            version: RemoteVersion("deadbeef".to_string()),
        }];
        let entries = mutations_to_tree_entries(&mutations);
        assert_eq!(entries[0]["sha"], "deadbeef");
        assert!(entries[0].get("content").is_none());
    }

    #[test]
    fn mutations_to_tree_entries_delete_uses_null_sha() {
        let mutations = vec![BatchMutation::Delete {
            path: "a/d.txt".to_string(),
        }];
        let entries = mutations_to_tree_entries(&mutations);
        assert_eq!(entries[0]["sha"], serde_json::Value::Null);
    }
}
