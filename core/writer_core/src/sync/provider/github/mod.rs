//! GitHub Provider — 基于 GitHub REST API 的 [`SyncProvider`] 实现。
//!
//! 本模块仅在 `github-api` feature 启用时编译。
//!
//! [`GitHubProvider`] 持有 [`GitHubRuntimeConfig`] + `SyncTransport`，
//! 实现 [`SyncProvider`] 的 list/read/write/delete 四个原语。
//! 所有 GitHub 特定逻辑（HTTP 状态映射、Contents API、Git tree API）封闭在此模块内，
//! 通用 LWW engine 只依赖 [`crate::sync::provider::SyncProvider`] trait。
//!
//! 持久化配置 [`GitHubProviderConfig`]（不含 token）存在 `SyncConfig.provider_config`，
//! 运行时配置 [`GitHubRuntimeConfig`]（含 token + 推导的 api_base_url）由
//! `GitHubRuntimeConfig::from_persisted` 在构造 Provider 时从持久化配置 + secrets 推导。

use std::sync::Arc;

use writer_platform_api::SyncTransport;

use self::config::GitHubRuntimeConfig;
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
/// 由 `GitHubRuntimeConfig` + `SyncTransport` 构造，实现 `SyncProvider` trait。
/// 所有远端路径都是完整远端路径（含 remote_prefix），Provider 内部直接拼 GitHub Contents API URL。
///
/// transport 用 `Arc` 存储以支持从共享引用克隆（旧 `GitHubApiBackend` 持有 `&self` 无法 move）。
pub struct GitHubProvider {
    config: GitHubRuntimeConfig,
    transport: Arc<dyn SyncTransport>,
}

impl GitHubProvider {
    /// 创建 GitHub Provider。
    ///
    /// `config` 携带 API base URL/token/branch 等运行时信息（含 token，不持久化），
    /// `transport` 为平台注入的 HTTP 客户端实现（用 `Arc` 共享所有权）。
    pub fn new(config: GitHubRuntimeConfig, transport: Arc<dyn SyncTransport>) -> Self {
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
        let api_base = &self.config.api_base_url;
        let token = &self.config.token;
        let branch = &self.config.branch;

        // 根据前置条件决定传给 GitHub PUT 的 SHA。
        //
        // - IfMatch(v)：严格使用调用方给的版本，不查远端；409 直接返回 PreconditionFailed。
        //   绝不在此分支"刷新 SHA"——冲突就是冲突，由 LWW engine 按乐观并发语义处理。
        // - CreateNew：不查旧 SHA，直接以 None 创建；对象已存在时 GitHub 返回 409/422。
        // - Unconditional：Provider 自己先读取当前远端 SHA，存在则用当前 SHA 覆盖，
        //   不存在则传 None 创建。这是唯一允许"刷新 SHA"的分支。
        //   注意：读取 SHA 后到写入之间的竞态是允许的（LWW 引擎会在下次同步时检测冲突）。
        let remote_sha = match &precondition {
            WritePrecondition::IfMatch(v) => Some(v.0.clone()),
            WritePrecondition::CreateNew => None,
            WritePrecondition::Unconditional => {
                client::get_content_sha(transport, api_base, token, branch, path)?
            }
        };

        let (status, body, new_sha) = client::put_content_serial(
            transport, api_base, token, branch, path, content, remote_sha,
        )?;

        if is_success_status(status) {
            // 新 SHA 从 PUT 响应中解析，避免写入后重新读取的竞态条件。
            // 若响应中未包含 SHA（异常情况），回退到重新读取。
            let sha = if let Some(sha) = new_sha {
                sha
            } else {
                // 回退：响应中无 SHA 时重新读取（理论上不应发生）
                client::get_content_sha(transport, api_base, token, branch, path)?
                    .unwrap_or_default()
            };
            return Ok(RemoteVersion(sha));
        }

        match status {
            409 => Err(ProviderError::PreconditionFailed {
                path: path.to_string(),
                reason: match precondition {
                    WritePrecondition::IfMatch(_) => {
                        format!("remote version changed: {}", truncate(&body, 200))
                    }
                    WritePrecondition::CreateNew => {
                        format!("object already exists: {}", truncate(&body, 200))
                    }
                    WritePrecondition::Unconditional => {
                        format!(
                            "conditional write required by server: {}",
                            truncate(&body, 200)
                        )
                    }
                },
            }),
            // CreateNew 时 GitHub 对已存在文件 PUT 不带 SHA 会返回 422（validation failed），
            // 映射为 PreconditionFailed，语义与 409 一致。
            422 if matches!(precondition, WritePrecondition::CreateNew) => {
                Err(ProviderError::PreconditionFailed {
                    path: path.to_string(),
                    reason: format!("object already exists: {}", truncate(&body, 200)),
                })
            }
            _ => Err(map_http_error(
                &format!("put contents {}", path),
                status,
                body,
            )),
        }
    }

    fn delete(&self, path: &str, precondition: DeletePrecondition) -> Result<(), ProviderError> {
        let transport = self.transport();
        let api_base = &self.config.api_base_url;
        let token = &self.config.token;
        let branch = &self.config.branch;

        // 根据前置条件决定传给 GitHub DELETE 的 SHA。
        //
        // - IfMatch(v)：严格使用调用方给的版本，不查远端；409 直接返回 PreconditionFailed。
        //   绝不在此分支"刷新 SHA"——冲突就是冲突，由 LWW engine 按乐观并发语义处理。
        // - Unconditional：Provider 自己先读取当前远端 SHA，存在则用当前 SHA 删除，
        //   不存在则直接成功（无需删除）。这是唯一允许"刷新 SHA"的分支。
        //   注意：读取 SHA 后到删除之间的竞态是允许的（LWW 引擎会在下次同步时检测冲突）。
        let remote_sha = match &precondition {
            DeletePrecondition::IfMatch(v) => Some(v.0.clone()),
            DeletePrecondition::Unconditional => {
                client::get_content_sha(transport, api_base, token, branch, path)?
            }
        };

        // 远端文件不存在（remote_sha 为 None）时直接成功。
        // 这只可能出现在 Unconditional 分支（IfMatch 拿到 None 不会走到这里，因为
        // IfMatch 总是 Some）；语义为"无条件删除一个不存在的对象 = 已达成目标"。
        let Some(sha) = remote_sha else {
            return Ok(());
        };

        let (status, body) =
            client::delete_content_once(transport, api_base, token, branch, path, &sha)?;

        if is_success_status(status) || status == 404 {
            return Ok(());
        }

        match status {
            409 => Err(ProviderError::PreconditionFailed {
                path: path.to_string(),
                reason: match precondition {
                    DeletePrecondition::IfMatch(_) => {
                        format!("remote version changed: {}", truncate(&body, 200))
                    }
                    DeletePrecondition::Unconditional => {
                        format!(
                            "conditional delete required by server: {}",
                            truncate(&body, 200)
                        )
                    }
                },
            }),
            _ => Err(map_http_error(
                &format!("delete contents {}", path),
                status,
                body,
            )),
        }
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
                result.error_category = "network".to_string();
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
        // Issue #645 评论 5504296097 第1点：GitHub 401/403 在 provider 层转成通用
        // AuthFailed/PermissionDenied code（from_code 仍兼容旧 token_invalid/
        // token_permission_denied 字符串）。
        let category = if status == 401 {
            "auth_failed"
        } else {
            "permission_denied"
        };
        result.error_category = category.to_string();
    } else if status == 404 {
        result.network_ok = true;
        result.network_status = "ok".to_string();
        result.auth_ok = true;
        result.auth_status = "ok".to_string();
        result.repo_ok = false;
        result.repo_status = "failed".to_string();
        result.error_category = "not_found".to_string();
    } else {
        result.network_ok = false;
        result.network_status = "failed".to_string();
        result.error_category = "network".to_string();
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

fn is_success_status(status: u16) -> bool {
    (200..300).contains(&status)
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
