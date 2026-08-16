//! 旧同步配置一次性迁移（Issue #630 评论第 4 点 / D）。
//!
//! # 背景
//!
//! 新 Core 只读：
//! - 全局 token：安全存储 `sync_token_global` / `sync_token_global_g<N>`
//! - 全局配置：`<app_data_root>/app-meta/sync/config.local.json`
//!
//! 旧版安全存储 key：
//! - 应用级：`sync_token_app` / `sync_token_app_g<N>`
//! - 作品级：`sync_token_<project_id>` / `sync_token_<project_id>_g<N>`
//! - 旧作品级配置：`<project_root>/app-meta/sync/config.local.json`
//!
//! 旧版已把 PAT 迁进安全存储并删除 `secrets.local.json` 后，新 Core 读不到。
//! 本模块做一次性只读探测 + 显式提交 + 旧凭据清理，失败/冲突时不删旧凭据。
//!
//! # 流程
//!
//! 1. 新全局 profile 已存在 → `NotNeeded`
//! 2. 探测旧应用级 profile（config + `sync_token_app[_gN]`）
//! 3. 探测旧作品级 profile（每个 project 的 config + `sync_token_<id>[_gN]`）
//! 4. 多个 profile 完全一致 → 迁一份；不一致 → `NeedsReconfigure`
//! 5. 提交到新全局（save config + `set_secret(sync_token_global, ...)`）
//! 6. 提交成功后删除旧 token / 旧 config 文件；失败时不删

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::sync::{SyncConfig, SyncSecrets};

/// 旧→新同步 profile 迁移结果。
#[derive(Debug, Clone, PartialEq)]
pub enum LegacyMigrationOutcome {
    /// 新全局已存在，无需迁移。
    NotNeeded,
    /// 迁移成功，已提交到全局并清理旧凭据。
    Migrated {
        config: SyncConfig,
        secrets: SyncSecrets,
    },
    /// 多项目旧 profile 冲突，需用户重选全局仓库。
    NeedsReconfigure { reason: String },
    /// 没找到任何可迁移的旧配置。
    NoLegacyConfig,
}

/// 安全存储 key 常量。
const GLOBAL_TOKEN_KEY: &str = "sync_token_global";
const LEGACY_APP_TOKEN_KEY: &str = "sync_token_app";
const LEGACY_PROJECT_TOKEN_KEY_PREFIX: &str = "sync_token_";
/// generation 枚举上限：探测 `sync_token_<base>_g1` .. `sync_token_<base>_g10`。
const MAX_LEGACY_GENERATION: u32 = 10;

/// 旧同步 profile 探测结果（内部用）。
#[derive(Debug, Clone)]
struct LegacyProfile {
    config: SyncConfig,
    token: String,
    /// 旧 token 在安全存储中的所有 key（成功后清理）。
    secret_keys: Vec<String>,
    /// 旧配置/凭据文件路径（成功后清理；不含与新全局共用的 app config）。
    files_to_cleanup: Vec<PathBuf>,
    /// 来源描述（冲突错误信息用）。
    source: String,
}

/// 旧→新同步 profile 迁移器。
///
/// 严格只读探测 + 显式提交；失败时不删旧凭据。
/// 不恢复旧产品 API/双同步正常路径，只保留旧格式只读迁移入口。
pub struct LegacySyncProfileMigrator<'a> {
    app_data_root: &'a Path,
    projects_root: &'a Path,
    secure_storage: Option<&'a dyn writer_platform_api::SecureStorage>,
}

impl<'a> LegacySyncProfileMigrator<'a> {
    pub fn new(
        app_data_root: &'a Path,
        projects_root: &'a Path,
        secure_storage: Option<&'a dyn writer_platform_api::SecureStorage>,
    ) -> Self {
        Self {
            app_data_root,
            projects_root,
            secure_storage,
        }
    }

    /// 一步完成迁移：探测 → 暂存 → 提交 → 清理旧。
    ///
    /// 失败/冲突时不删旧凭据。提交失败时返回 `Err`，调用方重试可再次调用。
    pub fn migrate(&self) -> Result<LegacyMigrationOutcome> {
        // 1. 新全局 profile 已存在 → NotNeeded
        if self.new_global_profile_exists() {
            return Ok(LegacyMigrationOutcome::NotNeeded);
        }

        // 2. 探测旧 profile（应用级 + 作品级）
        let mut profiles = self.detect_legacy_profiles()?;

        // 3. 根据探测结果决定
        let chosen = match profiles.as_slice() {
            [] => return Ok(LegacyMigrationOutcome::NoLegacyConfig),
            [single] => single.clone(),
            _ => {
                // 多个 profile：检查一致性
                let first = &profiles[0];
                if profiles.iter().all(|p| profiles_equivalent(p, first)) {
                    profiles.remove(0)
                } else {
                    return Ok(LegacyMigrationOutcome::NeedsReconfigure {
                        reason: describe_conflict(&profiles),
                    });
                }
            }
        };

        // 4. 提交到新全局（失败时直接返回 Err，不清理旧凭据）
        self.commit_to_global(&chosen)?;

        // 5. 提交成功后清理旧凭据（清理失败不阻塞迁移成功，只记日志）
        self.cleanup_legacy(&chosen);

        Ok(LegacyMigrationOutcome::Migrated {
            config: chosen.config,
            secrets: SyncSecrets {
                token: Some(chosen.token),
                ssh_private_key: None,
            },
        })
    }

    /// 新全局 profile 是否已存在（无需迁移）。
    ///
    /// 判据：app config 文件存在且 `remote_url` 非空，**且**
    /// `sync_token_global` 安全存储有非空值（或 fallback 文件有非空 token）。
    fn new_global_profile_exists(&self) -> bool {
        let config_path = self.app_data_root.join("app-meta/sync/config.local.json");
        let config = match load_sync_config_from(&config_path) {
            Some(c) => c,
            None => return false,
        };
        if config.remote_url.is_empty() {
            return false;
        }
        self.read_global_token().is_some()
    }

    /// 读新全局 token（安全存储 `sync_token_global` 或 fallback 文件）。
    fn read_global_token(&self) -> Option<String> {
        if let Some(storage) = self.secure_storage {
            if let Some(token) = read_nonempty_secret(storage, GLOBAL_TOKEN_KEY) {
                return Some(token);
            }
        }
        // fallback: <app_data_root>/app-meta/sync/secrets.local.json
        let secrets_path = self.app_data_root.join("app-meta/sync/secrets.local.json");
        read_token_from_secrets_file(&secrets_path)
    }

    /// 探测所有旧 profile（应用级 + 作品级）。
    fn detect_legacy_profiles(&self) -> Result<Vec<LegacyProfile>> {
        let mut profiles = Vec::new();

        // 1. 旧应用级
        if let Some(p) = self.detect_app_legacy_profile()? {
            profiles.push(p);
        }

        // 2. 旧作品级
        for project_id in self.list_project_ids()? {
            if let Some(p) = self.detect_project_legacy_profile(&project_id)? {
                profiles.push(p);
            }
        }

        Ok(profiles)
    }

    /// 探测旧应用级 profile。
    ///
    /// 旧应用级 config 路径与新全局相同（`<app_data_root>/app-meta/sync/config.local.json`），
    /// 但 token 在 `sync_token_app[_gN]`。若新全局 token 不存在但旧 app token 存在，
    /// 视为旧应用级 profile。
    fn detect_app_legacy_profile(&self) -> Result<Option<LegacyProfile>> {
        let config_path = self.app_data_root.join("app-meta/sync/config.local.json");
        let config = match load_sync_config_from(&config_path) {
            Some(c) => c,
            None => return Ok(None),
        };
        if config.remote_url.is_empty() {
            return Ok(None);
        }
        let (token, secret_keys, secret_files) =
            self.read_legacy_token(LEGACY_APP_TOKEN_KEY, self.app_data_root)?;
        let Some(token) = token else {
            return Ok(None);
        };
        if token.is_empty() {
            return Ok(None);
        }
        Ok(Some(LegacyProfile {
            config,
            token,
            secret_keys,
            // 旧应用级 config 路径与新全局相同，提交时覆盖，不单独删除
            files_to_cleanup: secret_files,
            source: "app".to_string(),
        }))
    }

    /// 探测旧作品级 profile。
    fn detect_project_legacy_profile(&self, project_id: &str) -> Result<Option<LegacyProfile>> {
        let project_root = self.projects_root.join(project_id);
        let config_path = project_root.join("app-meta/sync/config.local.json");
        let config = match load_sync_config_from(&config_path) {
            Some(c) => c,
            None => return Ok(None),
        };
        if config.remote_url.is_empty() {
            return Ok(None);
        }
        let base_key = format!("{}{}", LEGACY_PROJECT_TOKEN_KEY_PREFIX, project_id);
        let (token, secret_keys, secret_files) =
            self.read_legacy_token(&base_key, &project_root)?;
        let Some(token) = token else {
            return Ok(None);
        };
        if token.is_empty() {
            return Ok(None);
        }
        Ok(Some(LegacyProfile {
            config,
            token,
            secret_keys,
            // 旧作品级 config + secrets 文件都删
            files_to_cleanup: {
                let mut files = secret_files;
                files.push(config_path);
                files
            },
            source: format!("project:{}", project_id),
        }))
    }

    /// 读旧 token：先安全存储 `base_key`，再 `base_key_g1..gN`，再 fallback 文件。
    ///
    /// 返回 `(token, secret_keys_to_cleanup, secret_files_to_cleanup)`。
    /// `secret_files_to_cleanup` 仅在走文件 fallback 时非空。
    #[allow(clippy::type_complexity)]
    fn read_legacy_token(
        &self,
        base_key: &str,
        root: &Path,
    ) -> Result<(Option<String>, Vec<String>, Vec<PathBuf>)> {
        // 1-2. 安全存储：base_key + base_key_g1..gN
        if let Some(storage) = self.secure_storage {
            if let Some((token, key)) = read_token_from_storage(storage, base_key) {
                return Ok((Some(token), vec![key], Vec::new()));
            }
        }

        // 3. fallback: <root>/app-meta/sync/secrets.local.json
        let secrets_path = &root.join("app-meta/sync/secrets.local.json");
        if let Some(token) = read_token_from_secrets_file(secrets_path) {
            return Ok((Some(token), Vec::new(), vec![secrets_path.clone()]));
        }

        // 4. fallback: <root>/app-meta/sync/secrets_g<N>.local.json
        for gen in 1..=MAX_LEGACY_GENERATION {
            let path = root
                .join("app-meta/sync")
                .join(format!("secrets_g{}.local.json", gen));
            if let Some(token) = read_token_from_secrets_file(&path) {
                return Ok((Some(token), Vec::new(), vec![path]));
            }
        }

        Ok((None, Vec::new(), Vec::new()))
    }

    /// 列出所有作品 ID（只读目录，不触发 git 初始化副作用）。
    fn list_project_ids(&self) -> Result<Vec<String>> {
        if !self.projects_root.exists() {
            return Ok(Vec::new());
        }
        let mut ids = Vec::new();
        for entry in std::fs::read_dir(self.projects_root)? {
            let entry = entry?;
            let ft = entry.file_type()?;
            let is_dir = ft.is_dir() || (ft.is_symlink() && entry.path().is_dir());
            if !is_dir {
                continue;
            }
            // 只要有 project.json 就认为是作品目录
            let meta_path = entry.path().join("project.json");
            if !meta_path.exists() {
                continue;
            }
            if let Some(name) = entry.file_name().to_str().filter(|n| !n.is_empty()) {
                ids.push(name.to_string());
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// 提交到新全局：保存 config + 写 `sync_token_global`。
    ///
    /// 失败时返回 Err，调用方不进入清理阶段，旧凭据保留。
    fn commit_to_global(&self, profile: &LegacyProfile) -> Result<()> {
        // 1. 保存 config（覆盖旧应用级 config，路径相同）
        let config_path = self.app_data_root.join("app-meta/sync/config.local.json");
        save_config_atomic(&config_path, &profile.config)?;

        // 2. 写 token 到 sync_token_global
        if let Some(storage) = self.secure_storage {
            storage
                .set_secret(GLOBAL_TOKEN_KEY, profile.token.as_bytes())
                .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
        } else {
            // fallback: 写文件
            let secrets_path = self.app_data_root.join("app-meta/sync/secrets.local.json");
            let secrets = SyncSecrets {
                token: Some(profile.token.clone()),
                ssh_private_key: None,
            };
            write_secrets_atomic(&secrets_path, &secrets, "sync_secrets")?;
        }

        Ok(())
    }

    /// 清理旧凭据（安全存储 key + 旧 config/secrets 文件）。
    ///
    /// 清理失败不阻塞迁移成功（提交已完成），只记日志。
    /// 这样即使某个旧 key 删除失败，新全局已可用，下次迁移会走 NotNeeded 分支。
    fn cleanup_legacy(&self, profile: &LegacyProfile) {
        if let Some(storage) = self.secure_storage {
            for key in &profile.secret_keys {
                delete_secret_or_warn(storage, key);
            }
        }
        for file in &profile.files_to_cleanup {
            remove_file_or_warn(file);
        }
    }
}

/// 从安全存储读非空 token：先 base_key，再 base_key_g1..gN。
/// 返回 `(token, key)`，key 用于后续清理。
fn read_token_from_storage(
    storage: &dyn writer_platform_api::SecureStorage,
    base_key: &str,
) -> Option<(String, String)> {
    if let Some(token) = read_nonempty_secret(storage, base_key) {
        return Some((token, base_key.to_string()));
    }
    for gen in 1..=MAX_LEGACY_GENERATION {
        let key = format!("{}_g{}", base_key, gen);
        if let Some(token) = read_nonempty_secret(storage, &key) {
            return Some((token, key));
        }
    }
    None
}

/// 读安全存储 key 的非空 UTF-8 token。
fn read_nonempty_secret(
    storage: &dyn writer_platform_api::SecureStorage,
    key: &str,
) -> Option<String> {
    let bytes = storage.get_secret(key).ok().flatten()?;
    let token = String::from_utf8(bytes).ok()?;
    (!token.is_empty()).then_some(token)
}

/// 从 secrets 文件读非空 token（文件不存在或解析失败返回 None）。
fn read_token_from_secrets_file(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let secrets: SyncSecrets = serde_json::from_str(&content).ok()?;
    secrets.token.filter(|t| !t.is_empty())
}

/// 删除安全存储 key，失败时记日志（不阻塞迁移成功）。
fn delete_secret_or_warn(storage: &dyn writer_platform_api::SecureStorage, key: &str) {
    if let Err(e) = storage.delete_secret(key) {
        log::warn!("legacy migration: delete_secret({}) failed: {}", key, e);
    }
}

/// 删除文件，失败时记日志（不阻塞迁移成功）。
fn remove_file_or_warn(file: &Path) {
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
fn profiles_equivalent(a: &LegacyProfile, b: &LegacyProfile) -> bool {
    a.config.remote_url == b.config.remote_url
        && a.config.branch == b.config.branch
        && a.token == b.token
}

/// 描述多 profile 冲突原因（供 UI 提示用户重选）。
fn describe_conflict(profiles: &[LegacyProfile]) -> String {
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
fn load_sync_config_from(path: &Path) -> Option<SyncConfig> {
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<SyncConfig>(&content).ok()
}

/// 原子写入 sync config。
fn save_config_atomic(config_path: &Path, config: &SyncConfig) -> Result<()> {
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
    let tmp_path = config_path.with_extension("tmp");
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(tmp_path, config_path)?;
    Ok(())
}

/// 原子写入 secrets 文件（fallback 路径，无安全存储时用）。
fn write_secrets_atomic(
    secrets_path: &Path,
    secrets: &SyncSecrets,
    tmp_prefix: &str,
) -> Result<()> {
    if let Some(parent) = secrets_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(secrets)
        .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
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

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;
    use writer_platform_api::SecureStorage;

    /// 桩安全存储：HashMap 存 key→bytes，可注入 set_secret 失败。
    struct StubSecureStorage {
        map: Mutex<HashMap<String, Vec<u8>>>,
        set_fails: Mutex<bool>,
    }

    impl StubSecureStorage {
        fn new() -> Self {
            Self {
                map: Mutex::new(HashMap::new()),
                set_fails: Mutex::new(false),
            }
        }

        fn inject_set_failure(&self) {
            *self.set_fails.lock().expect("set_fails poisoned") = true;
        }

        fn contains_key(&self, key: &str) -> bool {
            self.map.lock().expect("map poisoned").contains_key(key)
        }
    }

    impl writer_platform_api::SecureStorage for StubSecureStorage {
        fn get_secret(&self, key: &str) -> std::result::Result<Option<Vec<u8>>, String> {
            Ok(self.map.lock().expect("map poisoned").get(key).cloned())
        }
        fn set_secret(&self, key: &str, value: &[u8]) -> std::result::Result<(), String> {
            if *self.set_fails.lock().expect("set_fails poisoned") {
                return Err("injected set_secret failure".to_string());
            }
            self.map
                .lock()
                .expect("map poisoned")
                .insert(key.to_string(), value.to_vec());
            Ok(())
        }
        fn delete_secret(&self, key: &str) -> std::result::Result<(), String> {
            self.map.lock().expect("map poisoned").remove(key);
            Ok(())
        }
    }

    fn sample_config(remote_url: &str, branch: &str) -> SyncConfig {
        SyncConfig {
            enabled: true,
            backend_type: crate::sync::BackendType::GithubApi,
            remote_url: remote_url.to_string(),
            transport: crate::sync::SyncProtocol::HttpsToken,
            branch: branch.to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        }
    }

    fn write_config_file(root: &Path, config: &SyncConfig) {
        let path = root.join("app-meta/sync/config.local.json");
        std::fs::create_dir_all(path.parent().expect("parent exists")).expect("mkdir");
        let content = serde_json::to_string_pretty(config).expect("serialize");
        std::fs::write(&path, content).expect("write config");
    }

    fn write_project_meta(projects_root: &Path, project_id: &str) {
        let dir = projects_root.join(project_id);
        std::fs::create_dir_all(&dir).expect("mkdir project");
        std::fs::write(
            dir.join("project.json"),
            format!(
                r#"{{"id":"{}","title":"{}","created_at":"2024-01-01T00:00:00Z","updated_at":"2024-01-01T00:00:00Z","order":0}}"#,
                project_id, project_id
            ),
        )
        .expect("write project.json");
    }

    struct TestEnv {
        _tmp: TempDir,
        app_data_root: PathBuf,
        projects_root: PathBuf,
        storage: Arc<StubSecureStorage>,
    }

    impl TestEnv {
        fn new() -> Self {
            let tmp = tempfile::tempdir().expect("tempdir");
            let app_data_root = tmp.path().join("app-data");
            let projects_root = tmp.path().join("projects");
            std::fs::create_dir_all(&app_data_root).expect("mkdir app-data");
            std::fs::create_dir_all(&projects_root).expect("mkdir projects");
            Self {
                _tmp: tmp,
                app_data_root,
                projects_root,
                storage: Arc::new(StubSecureStorage::new()),
            }
        }

        fn migrator(&self) -> LegacySyncProfileMigrator<'_> {
            LegacySyncProfileMigrator::new(
                &self.app_data_root,
                &self.projects_root,
                Some(self.storage.as_ref() as &dyn writer_platform_api::SecureStorage),
            )
        }
    }

    /// 1. 旧 app token/generation：新全局不存在；旧 `sync_token_app` 有值 + app 配置 → 迁移成功。
    #[test]
    fn test_legacy_app_token_migration() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/repo.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_app", b"legacy_app_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated {
                config: c,
                secrets: s,
            } => {
                assert_eq!(c.remote_url, "https://github.com/test/repo.git");
                assert_eq!(s.token.as_deref(), Some("legacy_app_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }

        // 新全局 token 已写入
        assert!(env.storage.contains_key("sync_token_global"));
        // 旧 app token 已删除
        assert!(!env.storage.contains_key("sync_token_app"));
    }

    /// 1b. 旧 app generation token：`sync_token_app_g3` 有值 → 迁移成功。
    #[test]
    fn test_legacy_app_generation_token_migration() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/repo.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_app_g3", b"gen3_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated { secrets: s, .. } => {
                assert_eq!(s.token.as_deref(), Some("gen3_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }
        assert!(env.storage.contains_key("sync_token_global"));
        assert!(!env.storage.contains_key("sync_token_app_g3"));
    }

    /// 2. 单项目迁移：新全局不存在；app 级无旧配置；一个 project 有配置 → 迁移成功。
    #[test]
    fn test_single_project_migration() {
        let env = TestEnv::new();
        write_project_meta(&env.projects_root, "proj1");
        let config = sample_config("https://github.com/test/proj1.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &config);
        env.storage
            .set_secret("sync_token_proj1", b"proj1_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated {
                config: c,
                secrets: s,
            } => {
                assert_eq!(c.remote_url, "https://github.com/test/proj1.git");
                assert_eq!(s.token.as_deref(), Some("proj1_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }
        assert!(env.storage.contains_key("sync_token_global"));
        assert!(!env.storage.contains_key("sync_token_proj1"));
        // 旧作品级 config 文件已删除
        let old_config = env
            .projects_root
            .join("proj1/app-meta/sync/config.local.json");
        assert!(!old_config.exists());
        // 新全局 config 已写入
        let new_config = env.app_data_root.join("app-meta/sync/config.local.json");
        assert!(new_config.exists());
    }

    /// 3. 多项目一致：两个 project 旧 profile 完全一致 → 迁一份。
    #[test]
    fn test_multiple_projects_consistent_migration() {
        let env = TestEnv::new();
        write_project_meta(&env.projects_root, "proj1");
        write_project_meta(&env.projects_root, "proj2");
        let config = sample_config("https://github.com/test/shared.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &config);
        write_config_file(&env.projects_root.join("proj2"), &config);
        env.storage
            .set_secret("sync_token_proj1", b"shared_token")
            .expect("set");
        env.storage
            .set_secret("sync_token_proj2", b"shared_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated {
                config: c,
                secrets: s,
            } => {
                assert_eq!(c.remote_url, "https://github.com/test/shared.git");
                assert_eq!(s.token.as_deref(), Some("shared_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }
        assert!(env.storage.contains_key("sync_token_global"));
    }

    /// 4. 不一致 NeedsReconfigure：两个 project 仓库或 token 不同 → 返回 NeedsReconfigure，不迁、不删旧凭据。
    #[test]
    fn test_multiple_projects_inconsistent_needs_reconfigure() {
        let env = TestEnv::new();
        write_project_meta(&env.projects_root, "proj1");
        write_project_meta(&env.projects_root, "proj2");
        let config1 = sample_config("https://github.com/test/repo1.git", "main");
        let config2 = sample_config("https://github.com/test/repo2.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &config1);
        write_config_file(&env.projects_root.join("proj2"), &config2);
        env.storage
            .set_secret("sync_token_proj1", b"token1")
            .expect("set");
        env.storage
            .set_secret("sync_token_proj2", b"token2")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::NeedsReconfigure { reason } => {
                assert!(reason.contains("conflicting"));
                assert!(reason.contains("project:proj1"));
                assert!(reason.contains("project:proj2"));
            }
            other => panic!("expected NeedsReconfigure, got {:?}", other),
        }
        // 旧 token 保留（未迁移）
        assert!(env.storage.contains_key("sync_token_proj1"));
        assert!(env.storage.contains_key("sync_token_proj2"));
        // 新全局未建立
        assert!(!env.storage.contains_key("sync_token_global"));
        let new_config = env.app_data_root.join("app-meta/sync/config.local.json");
        assert!(!new_config.exists());
    }

    /// 5. 迁移失败保留凭据：模拟 set_secret 返回 Err → 旧 token 仍在。
    #[test]
    fn test_migration_failure_preserves_credentials() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/repo.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_app", b"legacy_app_token")
            .expect("set");
        env.storage.inject_set_failure();

        let result = env.migrator().migrate();
        assert!(result.is_err(), "expected migration to fail");

        // 旧 token 仍在
        assert!(env.storage.contains_key("sync_token_app"));
        // 新全局未建立
        assert!(!env.storage.contains_key("sync_token_global"));
    }

    /// 6. 成功后清理：迁移成功并完整提交后 → 旧 `sync_token_app*` 被删除。
    #[test]
    fn test_successful_migration_cleans_up() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/repo.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_app", b"legacy_app_token")
            .expect("set");
        env.storage
            .set_secret("sync_token_app_g1", b"legacy_app_g1_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        assert!(matches!(outcome, LegacyMigrationOutcome::Migrated { .. }));

        // chosen 是 sync_token_app（先读到），其 key 被删
        assert!(!env.storage.contains_key("sync_token_app"));
        // sync_token_app_g1 未被选为 chosen，保留（可接受，新全局已建立）
        // 新全局已建立
        assert!(env.storage.contains_key("sync_token_global"));
    }

    /// 6b. 成功后清理作品级：旧 `sync_token_<id>*` 和旧 config 文件被删除。
    #[test]
    fn test_successful_project_migration_cleans_up() {
        let env = TestEnv::new();
        write_project_meta(&env.projects_root, "proj1");
        let config = sample_config("https://github.com/test/proj1.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &config);
        env.storage
            .set_secret("sync_token_proj1", b"proj1_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        assert!(matches!(outcome, LegacyMigrationOutcome::Migrated { .. }));

        assert!(!env.storage.contains_key("sync_token_proj1"));
        let old_config = env
            .projects_root
            .join("proj1/app-meta/sync/config.local.json");
        assert!(!old_config.exists());
    }

    /// 7. 新全局已存在 → NotNeeded：不读旧配置。
    #[test]
    fn test_new_global_exists_not_needed() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/existing.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_global", b"existing_global_token")
            .expect("set");
        // 同时存在旧 app token（应被忽略）
        env.storage
            .set_secret("sync_token_app", b"legacy_app_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        assert_eq!(outcome, LegacyMigrationOutcome::NotNeeded);

        // 旧 token 未被触碰
        assert!(env.storage.contains_key("sync_token_app"));
        assert!(env.storage.contains_key("sync_token_global"));
    }

    /// 8. 无任何旧配置 → NoLegacyConfig。
    #[test]
    fn test_no_legacy_config() {
        let env = TestEnv::new();
        let outcome = env.migrator().migrate().expect("migrate");
        assert_eq!(outcome, LegacyMigrationOutcome::NoLegacyConfig);
    }

    /// 9. fallback 文件路径：安全存储无 token，旧作品级 secrets.local.json 有 token → 迁移成功。
    ///
    /// app 级 secrets 文件路径与新全局 fallback 相同，会被 NotNeeded 拦截；
    /// 这里测作品级文件 fallback：app 级无任何东西，project 有 secrets.local.json。
    #[test]
    fn test_fallback_secrets_file_migration() {
        let env = TestEnv::new();
        write_project_meta(&env.projects_root, "proj1");
        let config = sample_config("https://github.com/test/proj1.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &config);
        // 写旧作品级 secrets 文件（不通过安全存储）
        let secrets_path = env
            .projects_root
            .join("proj1/app-meta/sync/secrets.local.json");
        std::fs::create_dir_all(secrets_path.parent().expect("parent")).expect("mkdir");
        std::fs::write(
            &secrets_path,
            r#"{"token":"file_token","ssh_private_key":null}"#,
        )
        .expect("write secrets");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated { secrets: s, .. } => {
                assert_eq!(s.token.as_deref(), Some("file_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }
        // 旧作品级 secrets 文件已删除
        assert!(!secrets_path.exists());
    }

    /// 10. app 级 + project 级同时存在且一致 → 迁一份（app 优先）。
    #[test]
    fn test_app_and_project_consistent_migration() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/shared.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_app", b"shared_token")
            .expect("set");
        write_project_meta(&env.projects_root, "proj1");
        write_config_file(&env.projects_root.join("proj1"), &config);
        env.storage
            .set_secret("sync_token_proj1", b"shared_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated { secrets: s, .. } => {
                assert_eq!(s.token.as_deref(), Some("shared_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }
    }

    /// 11. app 级 + project 级不一致 → NeedsReconfigure。
    #[test]
    fn test_app_and_project_inconsistent_needs_reconfigure() {
        let env = TestEnv::new();
        let app_config = sample_config("https://github.com/app/repo.git", "main");
        write_config_file(&env.app_data_root, &app_config);
        env.storage
            .set_secret("sync_token_app", b"app_token")
            .expect("set");
        write_project_meta(&env.projects_root, "proj1");
        let proj_config = sample_config("https://github.com/proj/repo.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &proj_config);
        env.storage
            .set_secret("sync_token_proj1", b"proj_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        assert!(matches!(
            outcome,
            LegacyMigrationOutcome::NeedsReconfigure { .. }
        ));
        // 旧凭据保留
        assert!(env.storage.contains_key("sync_token_app"));
        assert!(env.storage.contains_key("sync_token_proj1"));
    }
}
