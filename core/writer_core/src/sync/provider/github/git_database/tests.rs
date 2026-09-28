//! git_database.rs 的单元测试。
//!
//! 按仓库结构门禁（`tools/check_source_structure.py` 的
//! production-test-bloat 规则）从生产文件拆出，测试逻辑保持不变。
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
            .ok_or_else(|| TransportError::new("test", "canned responses exhausted".to_string()))
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

/// 6 步成功响应：ref → commit → blob → trees → commits → ref PATCH。
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
        json_response(201, r#"{"sha":"blob_put_a","url":"u"}"#),
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

/// Issue #761 评论 5828969186 问题 1：Put 必须走 `POST /git/blobs` + tree entry `sha`，
/// 不得把 base64 文本塞进 tree entry 的 `content`。
///
/// 一次 batch（1 Put + 1 ReuseVersion + 1 Delete）必须恰好产生 6 个请求，且不触碰 Contents API：
/// 1. GET ref → head commit；
/// 2. GET commit → base tree；
/// 3. POST blobs → Put 内容的 blob SHA（base64 + encoding:"base64"）；
/// 4. POST trees（base_tree + 全部 entry 用 sha 引用）；
/// 5. POST commits（parent=head，tree=新 tree）；
/// 6. PATCH ref（force=false）。
#[test]
fn commit_batch_runs_git_database_steps_in_order() {
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
    assert_eq!(
        requests.len(),
        6,
        "一次 batch（1 Put）恰好 6 个请求：ref/commit/blob/trees/commits/ref，实际 {}",
        requests.len()
    );
    let seen: Vec<(&str, String)> = requests
        .iter()
        .map(|r| (r.method.as_str(), r.url.clone()))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("GET", format!("{API_BASE}/git/ref/heads/main")),
            ("GET", format!("{API_BASE}/git/commits/head_c")),
            ("POST", format!("{API_BASE}/git/blobs")),
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

    // POST /git/blobs：内容是 base64 + encoding:"base64"（blob 接口的编码方式）。
    let blob = request_json(&requests[2]);
    assert_eq!(
        blob["content"], "aGVsbG8=",
        "blob content 应为文件内容的 base64"
    );
    assert_eq!(blob["encoding"], "base64");

    // POST /git/trees：base_tree + 三种 entry，全部用 sha 引用，绝不用 content。
    let trees = request_json(&requests[3]);
    assert_eq!(trees["base_tree"], "base_t");
    let entries = trees["tree"].as_array().expect("tree entries");
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0]["path"], "projects/p1/__generations__/g1/a.md");
    assert_eq!(entries[0]["mode"], "100644");
    assert_eq!(entries[0]["type"], "blob");
    assert_eq!(
        entries[0]["sha"], "blob_put_a",
        "Put 必须引用 POST /git/blobs 刚创建的 blob SHA"
    );
    assert!(
        entries[0].get("content").is_none(),
        "tree entry 不得带 content（Create a tree 的 content 是文件内容本身，不是 base64）"
    );
    assert_eq!(entries[1]["sha"], "blob_sha_b");
    assert!(entries[1].get("content").is_none());
    assert_eq!(entries[2]["sha"], serde_json::Value::Null);

    // POST /git/commits：新 tree + parent=head。
    let commit = request_json(&requests[4]);
    assert_eq!(commit["message"], "WriterApp publish generation g1");
    assert_eq!(commit["tree"], "new_t");
    assert_eq!(commit["parents"][0], "head_c");

    // PATCH /git/refs：新 commit + force=false（绝不覆盖别人刚提交的 head）。
    let patch = request_json(&requests[5]);
    assert_eq!(patch["sha"], "new_c");
    assert_eq!(patch["force"], false);
}

/// Issue #761 评论 5828969186 问题 1：UTF-8 正文必须以原始字节写入。
///
/// 回归保护：之前把 base64 文本当 tree `content` 提交，`你好` 会变成
/// `5L2g5aW9` 这串字面量。现在必须：
/// - blob body 的 content == UTF-8 字节的 base64；
/// - tree entry 只引用 blob SHA，不带任何 base64 文本。
#[test]
fn commit_batch_writes_utf8_content_as_blob_bytes_not_base64_text() {
    use base64::Engine;

    let text = "你好";
    let mut responses = success_responses();
    responses[2] = json_response(201, r#"{"sha":"blob_utf8","url":"u"}"#);
    let transport = CannedTransport::new(responses);
    let mutations = vec![BatchMutation::Put {
        path: "projects/p1/__generations__/g1/chapter.md".to_string(),
        content: text.as_bytes().to_vec(),
    }];

    commit_batch_via_git_database(&transport, API_BASE, "tok", "main", &mutations, "publish")
        .expect("batch should succeed");

    let requests = transport.requests();
    let blob = request_json(&requests[2]);
    assert_eq!(
        blob["content"],
        base64::engine::general_purpose::STANDARD.encode(text.as_bytes()),
        "blob content 必须是正文 UTF-8 字节的 base64"
    );
    let trees = request_json(&requests[3]);
    let entries = trees["tree"].as_array().expect("tree entries");
    assert_eq!(entries[0]["sha"], "blob_utf8");
    assert!(
        entries[0].get("content").is_none(),
        "tree entry 不得出现 base64 文本内容"
    );
    assert!(
        !trees["tree"].to_string().contains("5L2g5aW9"),
        "tree payload 里不得出现 base64(你好) 的字面量：{trees}"
    );
}

/// Issue #761 Part 2：ref 更新冲突（409/422）映射成 `PreconditionFailed`。
#[test]
fn commit_batch_maps_ref_conflict_to_precondition_failed() {
    for status in [409_u16, 422_u16] {
        let mut responses = success_responses();
        responses[5] = json_response(status, r#"{"message":"Update is not a fast forward"}"#);
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
            6,
            "ref 冲突发生在最后一步，前 5 步已执行"
        );
    }
}

/// Issue #761 Part 2：tree 创建失败时不创建 commit、不推进 ref。
#[test]
fn commit_batch_stops_before_commit_when_tree_creation_fails() {
    let mut responses = success_responses();
    responses[3] = json_response(422, r#"{"message":"Invalid tree entry"}"#);
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
        4,
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

/// Issue #761 评论 5828969186 问题 1：blob 创建失败时不创建 tree/commit/ref。
#[test]
fn commit_batch_stops_before_tree_when_blob_creation_fails() {
    let mut responses = success_responses();
    responses[2] = json_response(422, r#"{"message":"content is required"}"#);
    let transport = CannedTransport::new(responses);

    let err = commit_batch_via_git_database(
        &transport,
        API_BASE,
        "tok",
        "main",
        &sample_mutations(),
        "publish",
    )
    .expect_err("blob creation failure must fail the batch");

    assert!(
        !matches!(err, ProviderError::PreconditionFailed { .. }),
        "blob 创建失败不是 CAS 冲突，实际 {err:?}"
    );
    assert_eq!(
        transport.requests().len(),
        3,
        "blob 创建失败后不得创建 tree/commit/ref"
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
fn parse_blob_sha_extracts_top_level_sha() {
    let body = r#"{"sha":"blobnew","url":"https://api.github.com/repos/o/r/git/blobs/blobnew"}"#;
    assert_eq!(parse_blob_sha(body).unwrap(), "blobnew");
}

/// Issue #761 评论 5828969186 问题 1：Put 的 tree entry 引用刚创建的 blob SHA。
#[test]
fn mutations_to_tree_entries_put_uses_created_blob_sha() {
    let transport = CannedTransport::new(vec![json_response(201, r#"{"sha":"blob_x","url":"u"}"#)]);
    let mutations = vec![BatchMutation::Put {
        path: "a/b.txt".to_string(),
        content: b"hello".to_vec(),
    }];
    let entries = mutations_to_tree_entries(&transport, API_BASE, "tok", &mutations)
        .expect("blob upload should succeed");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["path"], "a/b.txt");
    assert_eq!(entries[0]["mode"], "100644");
    assert_eq!(entries[0]["type"], "blob");
    assert_eq!(entries[0]["sha"], "blob_x");
    assert!(
        entries[0].get("content").is_none(),
        "tree entry 不得携带 content"
    );

    // blob 上传请求体：base64 内容 + encoding:"base64"（内容是文件本身，不是 base64 文本）。
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url, format!("{API_BASE}/git/blobs"));
    let blob = request_json(&requests[0]);
    assert_eq!(blob["content"], "aGVsbG8=");
    assert_eq!(blob["encoding"], "base64");
}

#[test]
fn mutations_to_tree_entries_reuse_version_uses_sha_without_blob_upload() {
    let transport = CannedTransport::new(vec![]);
    let mutations = vec![BatchMutation::ReuseVersion {
        path: "a/c.txt".to_string(),
        version: RemoteVersion("deadbeef".to_string()),
    }];
    let entries = mutations_to_tree_entries(&transport, API_BASE, "tok", &mutations)
        .expect("reuse version needs no blob upload");
    assert_eq!(entries[0]["sha"], "deadbeef");
    assert!(entries[0].get("content").is_none());
    assert!(
        transport.requests().is_empty(),
        "ReuseVersion 不得重新上传 blob"
    );
}

#[test]
fn mutations_to_tree_entries_delete_uses_null_sha_without_blob_upload() {
    let transport = CannedTransport::new(vec![]);
    let mutations = vec![BatchMutation::Delete {
        path: "a/d.txt".to_string(),
    }];
    let entries = mutations_to_tree_entries(&transport, API_BASE, "tok", &mutations)
        .expect("delete needs no blob upload");
    assert_eq!(entries[0]["sha"], serde_json::Value::Null);
    assert!(transport.requests().is_empty(), "Delete 不得上传 blob");
}
