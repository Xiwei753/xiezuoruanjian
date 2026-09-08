//! 旧同步配置一次性迁移（Issue #630 评论第 4 点 / D、第 5 点 Part C）。
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
//! 2. 优先探测旧应用级 profile（config + `sync_token_app` 或精确 generation key）
//!    - 有旧 app profile → 直接迁移，不检查 project（app 优先级高于 project）
//! 3. 无 app profile 时探测旧作品级 profile（每个 project 的 config + token）
//!    - 多个 project 互相一致 → 迁一份；不一致 → `NeedsReconfigure`
//! 4. 提交到新全局（save config + `set_secret(sync_token_global, ...)`）
//! 5. 提交成功后对所有参与迁移的等价旧 profile 删除旧 token / 旧 config 文件；失败时不删
//!
//! # generation 处理
//!
//! 旧版安全存储可能存在 `sync_token_<base>_g<N>` 形式的 generation key。
//! 旧 profile 的 generation 完全可能大于 10，因此本模块不再猜测枚举上限，
//! 而是接受调用方提供的精确 `LegacyProfileMetadata::active_generation`：
//! - `Some(n)` → 精确读取 `sync_token_<base>_g{n}`
//! - `None` → 回退 base key / `secrets.local.json` 文件（旧 DataStore 无 committed generation）
//!
//! # 模块拆分
//!
//! - 本文件：迁移器（探测 / 提交 / 清理）与公开 DTO；
//! - [`legacy_migration_io`]：安全存储 / 文件 IO 纯函数（token 读写、config/secrets 原子写）；
//! - [`legacy_migration_tests`]（cfg(test)）：迁移行为测试。

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::sync::{SyncConfig, SyncSecrets};

mod legacy_migration_io;

#[cfg(test)]
#[cfg(feature = "github-api")]
mod legacy_migration_tests;

pub(crate) use legacy_migration_io::{
    delete_secret_or_warn, describe_conflict, load_sync_config_from, profiles_equivalent,
    read_nonempty_secret, read_token_from_secrets_file, read_token_from_storage,
    remove_file_or_warn, save_config_atomic, write_secrets_atomic,
};

/// 旧→新同步 profile 迁移结果。
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
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

/// 旧 profile 的精确 generation metadata（Issue #630 评论第 5 点 Part C）。
///
/// 调用方（平台层 DataStore）知道每个旧 profile 当前 committed 的 generation，
/// 通过此结构精确告诉 Core 应该读取哪个 `sync_token_<base>_g<N>` key，
/// 避免 Core 猜测枚举上限（旧 generation 完全可能大于 10）。
///
/// - `source = "app"`：旧应用级 profile；`project_id` 应为 None
/// - `source = "project:<id>"`：旧作品级 profile；`project_id` 应为 Some(id)
/// - `active_generation = Some(n)`：精确读取 `sync_token_<base>_g{n}`
/// - `active_generation = None`：旧 DataStore 无 committed generation，回退 base key / 文件
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LegacyProfileMetadata {
    /// 来源描述：`"app"` 或 `"project:<id>"`。
    pub source: String,
    /// 作品 ID（仅 project source 有意义；app source 为 None）。
    pub project_id: Option<String>,
    /// 精确的 committed generation；None 表示无 generation 信息，回退 base key / 文件。
    pub active_generation: Option<u32>,
}

/// 旧同步 profile 探测结果（内部用）。
#[derive(Debug, Clone)]
pub(crate) struct LegacyProfile {
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
    /// 无 metadata 的 fallback：只读 base key / `secrets.local.json` 文件，
    /// 不猜测 generation。当调用方知道精确 generation 时应使用
    /// [`migrate_with_metadata`](Self::migrate_with_metadata)。
    ///
    /// 失败/冲突时不删旧凭据。提交失败时返回 `Err`，调用方重试可再次调用。
    pub fn migrate(&self) -> Result<LegacyMigrationOutcome> {
        self.migrate_with_metadata(&[])
    }

    /// 一步完成迁移，接受调用方提供的精确 generation metadata。
    ///
    /// metadata 中每个 [`LegacyProfileMetadata`] 描述一个旧 profile 的 source 和
    /// committed generation。当 `active_generation = Some(n)` 时精确读取
    /// `sync_token_<base>_g{n}`；当 `active_generation = None` 时回退 base key / 文件。
    ///
    /// 流程：
    /// 1. 新全局已存在 → `NotNeeded`
    /// 2. 优先探测旧 app profile（不检查 project，app 优先级高于 project）
    /// 3. 无 app profile 时探测旧 project profiles，多个 project 互相不一致 → `NeedsReconfigure`
    /// 4. 提交成功后清理所有参与迁移的等价旧 profile；失败/冲突时不清理
    pub fn migrate_with_metadata(
        &self,
        metadata: &[LegacyProfileMetadata],
    ) -> Result<LegacyMigrationOutcome> {
        // 1. 新全局 profile 已存在 → NotNeeded
        if self.new_global_profile_exists() {
            return Ok(LegacyMigrationOutcome::NotNeeded);
        }

        // 2. 优先探测旧 app profile（不检查 project）
        let app_metadata = metadata.iter().find(|m| m.source == "app");
        if let Some(app_profile) = self.detect_app_legacy_profile(app_metadata)? {
            // app 优先：直接迁移，不比较 project
            return self.execute_migration(&app_profile, std::slice::from_ref(&app_profile));
        }

        // 3. 无 app profile → 探测旧 project profiles，只比较 project ↔ project
        let project_profiles = self.detect_project_legacy_profiles(metadata)?;

        // 4. 根据探测结果决定
        match project_profiles.as_slice() {
            [] => Ok(LegacyMigrationOutcome::NoLegacyConfig),
            [single] => self.execute_migration(single, std::slice::from_ref(single)),
            _ => {
                let first = &project_profiles[0];
                if project_profiles
                    .iter()
                    .all(|p| profiles_equivalent(p, first))
                {
                    // 多个 project 一致：迁一份，成功后清理全部参与 profile
                    self.execute_migration(first, &project_profiles)
                } else {
                    Ok(LegacyMigrationOutcome::NeedsReconfigure {
                        reason: describe_conflict(&project_profiles),
                    })
                }
            }
        }
    }

    /// 新全局 profile 是否已存在（无需迁移）。
    ///
    /// 判据：app config 文件存在且 GitHub `remote_url` 非空，**且**
    /// `sync_token_global` 安全存储有非空值（或 fallback 文件有非空 token）。
    fn new_global_profile_exists(&self) -> bool {
        let config_path = self.app_data_root.join("app-meta/sync/config.local.json");
        let config = match load_sync_config_from(&config_path) {
            Some(c) => c,
            None => return false,
        };
        if github_remote_url_from_config(&config).is_empty() {
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

    /// 探测旧应用级 profile。
    ///
    /// 旧应用级 config 路径与新全局相同（`<app_data_root>/app-meta/sync/config.local.json`），
    /// 但 token 在 `sync_token_app` 或 `sync_token_app_g<N>`（由 metadata 精确指定）。
    /// 若新全局 token 不存在但旧 app token 存在，视为旧应用级 profile。
    ///
    /// `metadata.active_generation`：
    /// - `Some(n)` → 精确读 `sync_token_app_g{n}`
    /// - `None` → 读 `sync_token_app` 或 fallback 文件
    fn detect_app_legacy_profile(
        &self,
        metadata: Option<&LegacyProfileMetadata>,
    ) -> Result<Option<LegacyProfile>> {
        let config_path = self.app_data_root.join("app-meta/sync/config.local.json");
        let config = match load_sync_config_from(&config_path) {
            Some(c) => c,
            None => return Ok(None),
        };
        if github_remote_url_from_config(&config).is_empty() {
            return Ok(None);
        }
        let precise_gen = metadata.and_then(|m| m.active_generation);
        let (token, secret_keys, secret_files) =
            self.read_legacy_token(LEGACY_APP_TOKEN_KEY, self.app_data_root, precise_gen)?;
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

    /// 探测所有旧作品级 profile（不包含 app）。
    ///
    /// 对每个作品目录，查找 metadata 中 `source = "project:<id>"` 的项以获取精确 generation。
    fn detect_project_legacy_profiles(
        &self,
        metadata: &[LegacyProfileMetadata],
    ) -> Result<Vec<LegacyProfile>> {
        let mut profiles = Vec::new();
        for project_id in self.list_project_ids()? {
            let proj_metadata = metadata
                .iter()
                .find(|m| m.source == format!("project:{}", project_id));
            if let Some(p) = self.detect_project_legacy_profile(&project_id, proj_metadata)? {
                profiles.push(p);
            }
        }
        Ok(profiles)
    }

    /// 探测旧作品级 profile。
    ///
    /// `metadata.active_generation`：
    /// - `Some(n)` → 精确读 `sync_token_<id>_g{n}`
    /// - `None` → 读 `sync_token_<id>` 或 fallback 文件
    fn detect_project_legacy_profile(
        &self,
        project_id: &str,
        metadata: Option<&LegacyProfileMetadata>,
    ) -> Result<Option<LegacyProfile>> {
        let project_root = self.projects_root.join(project_id);
        let config_path = project_root.join("app-meta/sync/config.local.json");
        let config = match load_sync_config_from(&config_path) {
            Some(c) => c,
            None => return Ok(None),
        };
        if github_remote_url_from_config(&config).is_empty() {
            return Ok(None);
        }
        let base_key = format!("{}{}", LEGACY_PROJECT_TOKEN_KEY_PREFIX, project_id);
        let precise_gen = metadata.and_then(|m| m.active_generation);
        let (token, secret_keys, secret_files) =
            self.read_legacy_token(&base_key, &project_root, precise_gen)?;
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

    /// 读旧 token：根据 `precise_generation` 精确读取或回退 base key / 文件。
    ///
    /// - `precise_generation = Some(n)` → 只读 `base_key_g{n}`（精确）
    /// - `precise_generation = None` → 读 `base_key`，再回退 `secrets.local.json` 文件
    ///
    /// 返回 `(token, secret_keys_to_cleanup, secret_files_to_cleanup)`。
    /// `secret_files_to_cleanup` 仅在走文件 fallback 时非空。
    #[allow(clippy::type_complexity)]
    fn read_legacy_token(
        &self,
        base_key: &str,
        root: &Path,
        precise_generation: Option<u32>,
    ) -> Result<(Option<String>, Vec<String>, Vec<PathBuf>)> {
        // 1. 安全存储：精确 generation key 或 base key
        if let Some(storage) = self.secure_storage {
            if let Some((token, key)) =
                read_token_from_storage(storage, base_key, precise_generation)
            {
                return Ok((Some(token), vec![key], Vec::new()));
            }
        }

        // 2. 文件 fallback：仅在无精确 generation 时回退 base key 文件
        if precise_generation.is_none() {
            let secrets_path = &root.join("app-meta/sync/secrets.local.json");
            if let Some(token) = read_token_from_secrets_file(secrets_path) {
                return Ok((Some(token), Vec::new(), vec![secrets_path.clone()]));
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

    /// 执行迁移：先提交到新全局，成功后清理所有参与旧 profile。
    ///
    /// - `chosen`：用于提交的 profile（多个等价 profile 中选第一个）
    /// - `all_participating`：所有参与迁移的等价旧 profile（成功后全部清理）
    ///
    /// 提交失败时返回 Err，不清理任何旧凭据。
    fn execute_migration(
        &self,
        chosen: &LegacyProfile,
        all_participating: &[LegacyProfile],
    ) -> Result<LegacyMigrationOutcome> {
        // 1. 提交到新全局（失败时直接返回 Err，不清理旧凭据）
        self.commit_to_global(chosen)?;

        // 2. 提交成功后清理所有参与迁移的旧凭据（清理失败不阻塞迁移成功，只记日志）
        for profile in all_participating {
            self.cleanup_legacy(profile);
        }

        Ok(LegacyMigrationOutcome::Migrated {
            config: chosen.config.clone(),
            secrets: SyncSecrets::from_github_token(chosen.token.clone()),
        })
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
            let secrets = SyncSecrets::from_github_token(profile.token.clone());
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

/// 从 `SyncConfig` 读取 GitHub `remote_url`（若为 GitHub provider）；否则空字符串。
///
/// Issue #645 评论第 2 点：`remote_url` 不再在 `SyncConfig` 顶层，
/// 从 `provider_config: ProviderConfig::GitHub` 读取。
fn github_remote_url_from_config(config: &SyncConfig) -> String {
    match &config.provider_config {
        #[cfg(feature = "github-api")]
        Some(crate::sync::provider::ProviderConfig::GitHub(gh)) => gh.remote_url.clone(),
        _ => String::new(),
    }
}
