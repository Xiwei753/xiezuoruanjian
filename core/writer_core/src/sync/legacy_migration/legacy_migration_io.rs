//! 旧同步配置迁移的 IO 纯函数（安全存储 / 文件读写）。
//!
//! 从 [`super::legacy_migration`] 拆出：token 读写、config/secrets 原子写、
//! profile 等价性与冲突描述。全部是无状态函数，便于独立测试。

use std::path::Path;

use crate::error::Result;
use crate::sync::{SyncConfig, SyncSecrets};

use super::LegacyProfile;

/// 从安全存储读非空 token。
///
/// - `precise_generation = Some(n)` → 读 `base_key_g{n}`
/// - `precise_generation = None` → 读 `base_key`
///
/// 返回 `(token, key)`，key 用于后续清理。
pub(crate) fn read_token_from_storage(
    storage: &dyn writer_platform_api::SecureStorage,
    base_key: &str,
    precise_generation: Option<u32>,
) -> Option<(String, String)> {
    let key = match precise_generation {
        Some(gen) => format!("{}_g{}", base_key, gen),
        None => base_key.to_string(),
    };
    read_nonempty_secret(storage, &key).map(|token| (token, key))
}

/// 读安全存储 key 的非空 UTF-8 token。
pub(crate) fn read_nonempty_secret(
    storage: &dyn writer_platform_api::SecureStorage,
    key: &str,
) -> Option<String> {
    let bytes = storage.get_secret(key).ok().flatten()?;
    let token = String::from_utf8(bytes).ok()?;
    (!token.is_empty()).then_some(token)
}

/// 从 secrets 文件读非空 token（文件不存在或解析失败返回 None）。
pub(crate) fn read_token_from_secrets_file(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let secrets: SyncSecrets = serde_json::from_str(&content).ok()?;
    secrets.token.filter(|t| !t.is_empty())
}

/// 删除安全存储 key，失败时记日志（不阻塞迁移成功）。
pub(crate) fn delete_secret_or_warn(storage: &dyn writer_platform_api::SecureStorage, key: &str) {
    if let Err(e) = storage.delete_secret(key) {
        log::warn!("legacy migration: delete_secret({}) failed: {}", key, e);
    }
}

/// 删除文件，失败时记日志（不阻塞迁移成功）。
pub(crate) fn remove_file_or_warn(file: &Path) {
    if file.exists() {
        if let Err(e) = std::fs::remove_file(file) {
            log::warn!(
                "legacy migration: remove_file({}) failed: {}",
                file.display(),
                e
            );
        }
    }
}

/// 两个旧 profile 是否完全一致（仓库 + branch + token）。
pub(crate) fn profiles_equivalent(a: &LegacyProfile, b: &LegacyProfile) -> bool {
    a.config.remote_url == b.config.remote_url
        && a.config.branch == b.config.branch
        && a.token == b.token
}

/// 描述多 profile 冲突原因（供 UI 提示用户重选）。
pub(crate) fn describe_conflict(profiles: &[LegacyProfile]) -> String {
    let mut summary = Vec::new();
    for p in profiles {
        summary.push(format!(
            "source={}, remote_url={}, branch={}, token_len={}",
            p.source,
            p.config.remote_url,
            p.config.branch,
            p.token.len()
        ));
    }
    format!(
        "multiple legacy sync profiles with conflicting repo/branch/token: [{}]",
        summary.join("; ")
    )
}

/// 从指定路径加载 SyncConfig（文件不存在或解析失败返回 None）。
pub(crate) fn load_sync_config_from(path: &Path) -> Option<SyncConfig> {
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<SyncConfig>(&content).ok()
}

/// 原子写入 sync config。
pub(crate) fn save_config_atomic(config_path: &Path, config: &SyncConfig) -> Result<()> {
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| crate::error::Error::Io(std::io::Error::other(e.to_string())))?;
    let tmp_path = config_path.with_extension("tmp");
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(tmp_path, config_path)?;
    Ok(())
}

/// 原子写入 secrets 文件（fallback 路径，无安全存储时用）。
pub(crate) fn write_secrets_atomic(
    secrets_path: &Path,
    secrets: &SyncSecrets,
    tmp_prefix: &str,
) -> Result<()> {
    if let Some(parent) = secrets_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(secrets)
        .map_err(|e| crate::error::Error::Io(std::io::Error::other(e.to_string())))?;
    let parent = secrets_path.parent().unwrap_or_else(|| Path::new(""));
    let mut tmp_file = tempfile::Builder::new()
        .prefix(tmp_prefix)
        .suffix(".tmp")
        .tempfile_in(parent)?;
    use std::io::Write;
    tmp_file.write_all(content.as_bytes())?;
    tmp_file.persist(secrets_path).map_err(|e| e.error)?;
    Ok(())
}
