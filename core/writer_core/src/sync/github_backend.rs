#![allow(deprecated)]
use crate::sync::backends::build_http_client;
use crate::sync::backends::SyncBackend;
use crate::sync::service::SyncService;
use crate::sync::types::FirstSyncMode;
use crate::sync::types::NetworkProbeResult;
use crate::sync::types::SyncConfig;
use crate::sync::types::SyncDiagnosticsResult;
use crate::sync::types::SyncResult;
use crate::sync::types::SyncSecrets;
use crate::sync::types::SyncStatus;
use crate::sync::url::mask_token_in_url;
use crate::sync::url::sanitize_remote_url;
use std::path::Path;

pub struct GitHubApiBackend;

pub(crate) struct ProbedClient {
    pub(crate) client: reqwest::blocking::Client,
    pub(crate) mode: String,
}

impl GitHubApiBackend {
    fn classify_reqwest_error(e: &reqwest::Error) -> (String, String) {
        let msg = e.to_string().to_lowercase();
        if msg.contains("dns") || msg.contains("resolve") || msg.contains("name resolution") {
            ("dns_failed".to_string(), "无法解析域名".to_string())
        } else if msg.contains("ssl") || msg.contains("certificate") || msg.contains("tls") {
            ("tls_failed".to_string(), "SSL/TLS 握手失败".to_string())
        } else if msg.contains("connection refused") {
            (
                "connection_refused".to_string(),
                "连接被拒绝 (端口可能未开放)".to_string(),
            )
        } else if e.is_timeout() || msg.contains("timeout") {
            ("timeout".to_string(), "连接超时".to_string())
        } else {
            ("network_failed".to_string(), "网络请求失败".to_string())
        }
    }

    pub(crate) fn build_auto_client(
        config: &SyncConfig,
        secrets: &SyncSecrets,
        workspace_path: Option<&Path>,
    ) -> crate::Result<(ProbedClient, Vec<NetworkProbeResult>)> {
        let mut probe_summary = Vec::new();
        let token = secrets.token.clone().unwrap_or_default();
        let api_base = Self::api_base_url(&config.remote_url);
        let test_url = format!(
            "{}/rate_limit",
            if api_base.starts_with("https://api.github.com/repos/") {
                "https://api.github.com"
            } else {
                &api_base
            }
        );

        let mut candidates = match config.proxy_type.as_str() {
            "auto" | "" => vec![
                (
                    "direct".to_string(),
                    "none".to_string(),
                    "".to_string(),
                    0u16,
                ),
                (
                    "http_local_7890".to_string(),
                    "http".to_string(),
                    "127.0.0.1".to_string(),
                    7890u16,
                ),
                (
                    "socks5_local_7891".to_string(),
                    "socks5".to_string(),
                    "127.0.0.1".to_string(),
                    7891u16,
                ),
            ],
            "none" => vec![(
                "direct".to_string(),
                "none".to_string(),
                "".to_string(),
                0u16,
            )],
            "http" => {
                let host = if config.proxy_host.is_empty() {
                    "127.0.0.1"
                } else {
                    &config.proxy_host
                };
                let port = if config.proxy_port > 0 {
                    config.proxy_port
                } else {
                    7890
                };
                vec![(
                    format!("http_{}:{}", host, port),
                    "http".to_string(),
                    host.to_string(),
                    port,
                )]
            }
            "socks5" => {
                let host = if config.proxy_host.is_empty() {
                    "127.0.0.1"
                } else {
                    &config.proxy_host
                };
                let port = if config.proxy_port > 0 {
                    config.proxy_port
                } else {
                    7891
                };
                vec![(
                    format!("socks5_{}:{}", host, port),
                    "socks5".to_string(),
                    host.to_string(),
                    port,
                )]
            }
            _ => vec![
                (
                    "direct".to_string(),
                    "none".to_string(),
                    "".to_string(),
                    0u16,
                ),
                (
                    "http_local_7890".to_string(),
                    "http".to_string(),
                    "127.0.0.1".to_string(),
                    7890u16,
                ),
                (
                    "socks5_local_7891".to_string(),
                    "socks5".to_string(),
                    "127.0.0.1".to_string(),
                    7891u16,
                ),
            ],
        };

        if let Some(wp) = workspace_path {
            if let Ok(state) = SyncService::load_sync_state(wp) {
                if let Some(last_mode) = state.last_successful_network_mode {
                    if let Some(pos) = candidates.iter().position(|c| c.0 == last_mode) {
                        let c = candidates.remove(pos);
                        candidates.insert(0, c);
                    }
                }
            }
        }

        for (mode_name, p_type, p_host, p_port) in &candidates {
            let mut builder = reqwest::blocking::Client::builder()
                .user_agent("WriterApp/1.0")
                .timeout(std::time::Duration::from_secs(3));

            if *p_type != "none" && !p_host.is_empty() && *p_port > 0 {
                let proxy_url = match p_type.as_str() {
                    "http" => format!("http://{}:{}", p_host, p_port),
                    "socks5" => format!("socks5h://{}:{}", p_host, p_port),
                    _ => format!("http://{}:{}", p_host, p_port),
                };
                if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
                    builder = builder.proxy(proxy);
                }
            }

            match builder.build() {
                Ok(client) => {
                    let req = client
                        .get(&test_url)
                        .header("User-Agent", "WriterApp/1.0")
                        .header("Accept", "application/vnd.github+json");

                    let req = if !token.is_empty() {
                        req.header("Authorization", format!("Bearer {}", token))
                    } else {
                        req
                    };

                    match req.send() {
                        Ok(resp) => {
                            let status = resp.status().as_u16();
                            let msg = if status == 200 {
                                "网络连通测试成功".to_string()
                            } else {
                                format!("网络可达 (HTTP {})", status)
                            };
                            probe_summary.push(NetworkProbeResult {
                                mode: mode_name.clone(),
                                success: true,
                                status: "ok".to_string(),
                                message: msg,
                                raw_error: None,
                            });

                            let mut working_config = config.clone();
                            working_config.proxy_type = p_type.clone();
                            working_config.proxy_host = p_host.clone();
                            working_config.proxy_port = *p_port;
                            working_config.proxy_enabled = *p_type != "none";
                            let final_client = build_http_client(Some(&working_config))?;
                            return Ok((
                                ProbedClient {
                                    client: final_client,
                                    mode: mode_name.clone(),
                                },
                                probe_summary,
                            ));
                        }
                        Err(e) => {
                            let raw = e.to_string();
                            let sanitized = if !token.is_empty() {
                                raw.replace(&token, "***TOKEN***")
                            } else {
                                raw.clone()
                            };
                            let truncated = if sanitized.len() > 200 {
                                format!(
                                    "{}...[truncated {} bytes]",
                                    &sanitized[..200],
                                    sanitized.len() - 200
                                )
                            } else {
                                sanitized
                            };
                            let (status_code, msg) = Self::classify_reqwest_error(&e);
                            probe_summary.push(NetworkProbeResult {
                                mode: mode_name.clone(),
                                success: false,
                                status: status_code,
                                message: msg,
                                raw_error: Some(truncated),
                            });
                        }
                    }
                }
                Err(e) => {
                    probe_summary.push(NetworkProbeResult {
                        mode: mode_name.clone(),
                        success: false,
                        status: "client_build_failed".to_string(),
                        message: "无法构建 HTTP 客户端".to_string(),
                        raw_error: Some(e.to_string()),
                    });
                }
            }
        }

        let error_detail = probe_summary
            .iter()
            .map(|p| {
                let err_suffix = p
                    .raw_error
                    .as_ref()
                    .map(|e| format!(" ({})", e))
                    .unwrap_or_default();
                format!(
                    "  [{}] {}: {}{}",
                    if p.success { "OK" } else { "FAIL" },
                    p.mode,
                    p.message,
                    err_suffix
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        Err(crate::Error::Other(format!(
            "network_probe_failed: 所有网络路径探测均失败:\n{}",
            error_detail
        )))
    }

    pub(crate) fn api_base_url(remote_url: &str) -> String {
        let sanitized = sanitize_remote_url(remote_url).sanitized_url;
        if let Some(path) = sanitized.strip_prefix("https://github.com/") {
            let path = path.strip_suffix(".git").unwrap_or(path);
            format!("https://api.github.com/repos/{}", path)
        } else if let Some(path) = sanitized.strip_prefix("http://github.com/") {
            let path = path.strip_suffix(".git").unwrap_or(path);
            format!("https://api.github.com/repos/{}", path)
        } else {
            sanitized
        }
    }
}

impl SyncBackend for GitHubApiBackend {
    fn diagnose(
        &self,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncDiagnosticsResult> {
        let mut result = SyncDiagnosticsResult::new();
        result.backend_type = "github_api".to_string();
        result.remote_url_sanitized = sanitize_remote_url(&config.remote_url)
            .sanitized_url
            .clone();
        result.transport = "https".to_string();
        result.app_proxy_status = if config.proxy_enabled {
            "已启用".to_string()
        } else {
            "未启用".to_string()
        };

        if !config.android_has_internet_permission {
            result.user_message = None;
            result.error_category = "missing_permission".to_string();
            return Ok(result);
        }

        let token = secrets.token.clone().unwrap_or_default();
        if token.is_empty() {
            result.user_message = None;
            result.error_category = "token_missing".to_string();
            return Ok(result);
        }

        let api_base = Self::api_base_url(&config.remote_url);
        let _masked_url = mask_token_in_url(&api_base);

        let probed_res = Self::build_auto_client(config, secrets, None);
        let (client, mode, probe_summary) = match probed_res {
            Ok((p, summary)) => (p.client, p.mode, summary),
            Err(e) => {
                result.user_message = None;
                result.error_category = "network_probe_failed".to_string();
                return Ok(result);
            }
        };

        result.chosen_network_mode = Some(mode.clone());
        result.network_probe_summary = probe_summary;

        let api_url = format!("{}/git/ref/heads/{}", api_base, config.branch);

        match client
            .get(&api_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "WriterApp/1.0")
            .header("Accept", "application/vnd.github+json")
            .send()
        {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body = resp.text().unwrap_or_default();
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
                    result.user_message = None;
                } else if status == 401 || status == 403 {
                    result.network_ok = true;
                    result.network_status = "ok".to_string();
                    result.auth_ok = false;
                    result.auth_status = "failed".to_string();
                    result.error_category = if status == 401 {
                        "token_invalid"
                    } else {
                        "token_permission_denied"
                    }
                    .to_string();
                    result.user_message = None;
                    result.raw_error = Some(format!(
                        "HTTP {} (body truncated): {}",
                        status,
                        &body.chars().take(200).collect::<String>()
                    ));
                } else if status == 404 {
                    result.network_ok = true;
                    result.network_status = "ok".to_string();
                    result.auth_ok = true;
                    result.auth_status = "ok".to_string();
                    result.repo_ok = false;
                    result.repo_status = "failed".to_string();
                    result.error_category = "repo_not_found_or_no_permission".to_string();
                    result.user_message = None;
                    result.raw_error = Some(format!(
                        "HTTP {} (body truncated): {}",
                        status,
                        &body.chars().take(200).collect::<String>()
                    ));
                } else {
                    result.network_ok = false;
                    result.network_status = "failed".to_string();
                    result.error_category = "github_network_failed".to_string();
                    result.user_message = None;
                    result.raw_error = Some(format!(
                        "HTTP {} (body truncated): {}",
                        status,
                        &body.chars().take(200).collect::<String>()
                    ));
                }
            }
            Err(e) => {
                let err_msg = e.to_string().to_lowercase();
                result.raw_error = Some(e.to_string());
                if err_msg.contains("dns")
                    || err_msg.contains("resolve")
                    || err_msg.contains("name resolution")
                {
                    result.error_category = "dns_failed".to_string();
                    result.user_message = None;
                } else if err_msg.contains("ssl")
                    || err_msg.contains("certificate")
                    || err_msg.contains("tls")
                {
                    result.error_category = "tls_failed".to_string();
                    result.user_message = None;
                } else if err_msg.contains("connection refused")
                    || err_msg.contains("timeout")
                    || err_msg.contains("network unreachable")
                {
                    result.error_category = "github_network_failed".to_string();
                    result.user_message = None;
                } else {
                    result.error_category = "github_network_failed".to_string();
                    result.user_message = None;
                }
                result.network_ok = false;
                result.network_status = "failed".to_string();
            }
        }

        Ok(result)
    }

    fn pull(&self, _: &Path, _: &SyncConfig, _: &SyncSecrets) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            None,
            "GitHub API pull not implemented".to_string(),
        ))
    }

    fn push(&self, _: &Path, _: &SyncConfig, _: &SyncSecrets) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            None,
            "GitHub API push not implemented".to_string(),
        ))
    }

    fn sync(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncResult> {
        eprintln!(
            "[sync] backend_type=github_api sync_mode=lww_manifest entry=GitHubApiBackend::sync remote_url={}",
            mask_token_in_url(&sanitize_remote_url(&config.remote_url).sanitized_url)
        );
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SyncService::perform_lww_sync(workspace_path, config, secrets)
        })) {
            Ok(result) => result,
            Err(err) => {
                let panic_msg = if let Some(s) = err.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = err.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                Ok(SyncResult::error(
                    SyncStatus::FatalError("backend_panic".to_string()),
                    FirstSyncMode::NotAttempted,
                    None,
                    format!("backend_panic: {}", panic_msg),
                ))
            }
        }
    }
}
