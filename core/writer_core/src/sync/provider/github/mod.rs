//! GitHub Provider — 基于 GitHub REST API 的 [`SyncProvider`] 实现。
//!
//! 本模块仅在 `github-api` feature 启用时编译。
//!
//! [`GitHubProvider`] 持有 [`GitHubProviderConfig`] + `SyncTransport`，
//! 实现 [`SyncProvider`] 的 list/read/write/delete 四个原语。
//! 所有 GitHub 特定逻辑（HTTP 状态映射、Contents API、Git tree API）封闭在此模块内，
//! 通用 LWW engine 只依赖 [`crate::sync::provider::SyncProvider`] trait。

use std::sync::Arc;

use writer_platform_api::SyncTransport;

use self::config::GitHubProviderConfig;
use self::error::map_http_error;
use crate::sync::provider::capabilities::SyncCapabilities;
use crate::sync::provider::error::ProviderError;
use crate::sync::provider::model::{
    DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion, WritePrecondition,
};
use crate::sync::provider::SyncProvider;
use crate::sync::types::SyncDiagnosticsResult;
use crate::sync::url::sanitize_remote_url;

pub mod client;
pub mod config;
pub mod error;

/// GitHub Provider — 基于 GitHub REST API 的同步后端。
///
/// 由 `GitHubProviderConfig` + `SyncTransport` 构造，实现 `SyncProvider` trait。
/// 所有远端路径都是完整远端路径（含 remote_prefix），Provider 内部直接拼 GitHub Contents API URL。
///
/// transport 用 `Arc` 存储以支持从共享引用克隆（旧 `GitHubApiBackend` 持有 `&self` 无法 move）。
pub struct GitHubProvider {
    config: GitHubProviderConfig,
    transport: Arc<dyn SyncTransport>,
}

impl GitHubProvider {
    /// 创建 GitHub Provider。
    ///
    /// `config` 携带 API base URL/token/branch 等信息，
    /// `transport` 为平台注入的 HTTP 客户端实现（用 `Arc` 共享所有权）。
    pub fn new(config: GitHubProviderConfig, transport: Arc<dyn SyncTransport>) -> Self {
        Self { config, transport }
    }

    fn transport(&self) -> &dyn SyncTransport {
        self.transport.as_ref()
    }
}

impl SyncProvider for GitHubProvider {
    fn capabilities(&self) -> SyncCapabilities {
        SyncCapabilities::github()
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        let transport = self.transport();
        let api_base = &self.config.api_base_url;
        let token = &self.config.token;
        let branch = &self.config.branch;

        let resp = client::get_tree_recursive(transport, api_base, token, branch)?;
        let status = resp.status;
        let body = String::from_utf8(resp.body).unwrap_or_default();

        if status == 404 {
            // tree 404 需区分：空仓库（ref 200）/ 分支不存在（ref 404 + repo 200）/ 权限不足。
            return diagnose_tree_404(transport, api_base, token, branch, prefix);
        }
        if !(200..300).contains(&status) {
            return Err(map_http_error("get recursive tree", status, body));
        }

        let json: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| ProviderError::Other {
                reason: format!("invalid tree json: {}", e),
            })?;
        let tree = json["tree"]
            .as_array()
            .ok_or_else(|| ProviderError::Other {
                reason: "tree response missing 'tree' array".to_string(),
            })?;

        let needle = format!("{prefix}/");
        let mut entries = Vec::new();
        for item in tree {
            // 只取 blob（文件），跳过 tree（目录）。
            if item["type"].as_str() != Some("blob") {
                continue;
            }
            let Some(path) = item["path"].as_str() else {
                continue;
            };
            let Some(sha) = item["sha"].as_str() else {
                continue;
            };
            if let Some(stripped) = path.strip_prefix(&needle) {
                entries.push(RemoteEntry {
                    path: stripped.to_string(),
                    version: RemoteVersion(sha.to_string()),
                });
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    fn read(&self, path: &str) -> Result<Option<RemoteObject>, ProviderError> {
        let transport = self.transport();
        let result = client::get_content(
            transport,
            &self.config.api_base_url,
            &self.config.token,
            &self.config.branch,
            path,
        )?;
        match result {
            Some((content, sha)) => Ok(Some(RemoteObject {
                path: path.to_string(),
                content,
                version: RemoteVersion(sha.unwrap_or_default()),
            })),
            None => Ok(None),
        }
    }

    fn write(
        &self,
        path: &str,
        content: &[u8],
        precondition: WritePrecondition,
    ) -> Result<RemoteVersion, ProviderError> {
        let transport = self.transport();
        let remote_sha = match precondition {
            WritePrecondition::IfMatch(v) => Some(v.0),
            WritePrecondition::CreateNew => None,
            WritePrecondition::Unconditional => None,
        };
        client::put_content_serial(
            transport,
            &self.config.api_base_url,
            &self.config.token,
            &self.config.branch,
            path,
            content,
            remote_sha,
        )?;
        // PUT 成功后重新拉取 sha 作为新版本标识。
        let new_sha = client::get_content_sha(
            transport,
            &self.config.api_base_url,
            &self.config.token,
            &self.config.branch,
            path,
        )?;
        Ok(RemoteVersion(new_sha.unwrap_or_default()))
    }

    fn delete(&self, path: &str, precondition: DeletePrecondition) -> Result<(), ProviderError> {
        let transport = self.transport();
        let remote_sha = match precondition {
            DeletePrecondition::IfMatch(v) => Some(v.0),
            DeletePrecondition::Unconditional => None,
        };
        client::delete_content_serial(
            transport,
            &self.config.api_base_url,
            &self.config.token,
            &self.config.branch,
            path,
            remote_sha,
        )
    }
}

impl GitHubProvider {
    /// 同步诊断 — 迁移自 `github_backend.rs` 的 diagnose 逻辑。
    ///
    /// 逐步检查网络、认证、仓库、分支可达性，结果填充到 `SyncDiagnosticsResult`。
    /// 即使某步失败也返回 `Ok(result)`，错误信息放在 `result.error_category` / `result.raw_error`，
    /// 供 UI 按步骤展示诊断链路。
    pub fn diagnose(&self) -> Result<SyncDiagnosticsResult, ProviderError> {
        let mut result = SyncDiagnosticsResult::new();
        result.backend_type = "github_api".to_string();
        result.remote_url_sanitized = sanitize_remote_url(&self.config.remote_url)
            .sanitized_url
            .clone();
        result.transport = "https".to_string();

        let transport = self.transport();
        let api_base = &self.config.api_base_url;
        let token = &self.config.token;
        let branch = &self.config.branch;

        let api_url = format!("{}/git/ref/heads/{}", api_base, branch);
        match client::execute_get(transport, &api_url, token) {
            Ok(resp) => {
                let status = resp.status;
                let body = String::from_utf8(resp.body).unwrap_or_default();
                apply_diagnose_status(&mut result, status, &body);
            }
            Err(e) => {
                let err_str = e.to_string();
                result.raw_error = Some(err_str.clone());
                if err_str.contains("dns") || err_str.contains("connect") {
                    result.error_category = "dns_failed".to_string();
                } else {
                    result.error_category = "github_network_failed".to_string();
                }
                result.network_ok = false;
                result.network_status = "failed".to_string();
            }
        }

        Ok(result)
    }
}

/// 根据 HTTP 状态码填充诊断结果（200/401/403/404/其他）。
///
/// 把 `diagnose` 内的状态分支拆出来，避免 `match` → `if` → `if/else` 三层嵌套。
fn apply_diagnose_status(result: &mut SyncDiagnosticsResult, status: u16, body: &str) {
    if status == 200 {
        result.success = true;
        result.network_ok = true;
        result.network_status = "ok".to_string();
        result.auth_ok = true;
        result.auth_status = "ok".to_string();
        result.repo_ok = true;
        result.repo_status = "ok".to_string();
        result.branch_ok = true;
        result.branch_status = "ok".to_string();
        return;
    }
    let truncated = body.chars().take(200).collect::<String>();
    result.raw_error = Some(format!("HTTP {} (body truncated): {}", status, truncated));
    if status == 401 || status == 403 {
        result.network_ok = true;
        result.network_status = "ok".to_string();
        result.auth_ok = false;
        result.auth_status = "failed".to_string();
        let category = if status == 401 {
            "token_invalid"
        } else {
            "token_permission_denied"
        };
        result.error_category = category.to_string();
    } else if status == 404 {
        result.network_ok = true;
        result.network_status = "ok".to_string();
        result.auth_ok = true;
        result.auth_status = "ok".to_string();
        result.repo_ok = false;
        result.repo_status = "failed".to_string();
        result.error_category = "repo_not_found_or_no_permission".to_string();
    } else {
        result.network_ok = false;
        result.network_status = "failed".to_string();
        result.error_category = "github_network_failed".to_string();
    }
}

/// 处理 `list` 中 tree API 返回 404 的诊断分支。
///
/// - ref 200 → 空仓库（分支存在但无 tree），返回空 Vec。
/// - ref 404 → 分支不存在，返回 `ProviderError::Other`（reason 标注 remote branch not found）。
/// - ref 401/403 → 认证/权限错误。
/// - ref 其他 → 网络错误。
fn diagnose_tree_404(
    transport: &dyn SyncTransport,
    api_base: &str,
    token: &str,
    branch: &str,
    prefix: &str,
) -> Result<Vec<RemoteEntry>, ProviderError> {
    let _ = prefix; // prefix 仅用于日志，此处不剥前缀。
    let ref_resp = client::get_ref(transport, api_base, token, branch)?;
    let ref_status = ref_resp.status;
    if (200..300).contains(&ref_status) {
        // 分支存在但 tree 404 → 空仓库。
        return Ok(Vec::new());
    }
    if ref_status == 404 {
        // 分支不存在；再查 repo 区分仓库不存在 vs 权限不足。
        let repo_resp = client::get_repo(transport, api_base, token)?;
        let repo_status = repo_resp.status;
        if (200..300).contains(&repo_status) {
            return Err(ProviderError::Other {
                reason: format!("remote branch not found: {}", branch),
            });
        }
        return Err(map_http_error(
            "get repo",
            repo_status,
            String::from_utf8(repo_resp.body).unwrap_or_default(),
        ));
    }
    Err(map_http_error(
        "get ref",
        ref_status,
        String::from_utf8(ref_resp.body).unwrap_or_default(),
    ))
}
