//! # 工作区管理（Core 层）
//!
//! 负责工作区的创建、验证、最近编辑记录。
//! 工作区是整个应用的根目录，包含所有项目和配置。
//!
//! ## 职责边界
//!
//! - **做**：创建目录结构、验证工作区有效性、记录最近编辑
//! - **不做**：项目内容管理、同步逻辑、设置管理（由其他模块负责）
//!
//! ## 目录结构
//!
//! ```text
//! workspace/
//!   workspace_manifest.json    # 工作区元数据
//!   projects/                  # 所有项目
//!   backups/                   # 备份文件
//!   trash/                     # 回收站
//!   app-meta/
//!     settings/                # 设置文件
//!     logs/                    # 日志
//!     sync/                    # 同步状态
//! ```

use crate::error::Result;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// 创建工作区目录结构和 manifest 文件。
///
/// 幂等操作：如果目录已存在则跳过。
pub fn create_workspace(path: &Path) -> Result<()> {
    fs::create_dir_all(path.join("app-meta/settings"))?;
    fs::create_dir_all(path.join("app-meta/logs"))?;
    fs::create_dir_all(path.join("projects"))?;
    fs::create_dir_all(path.join("backups"))?;
    fs::create_dir_all(path.join("trash"))?;
    fs::create_dir_all(path.join("sqlite_cache"))?;

    let manifest_path = path.join("workspace_manifest.json");
    if !manifest_path.exists() {
        crate::storage::atomic_write_string(&manifest_path, r#"{"version": 1}"#)?;
    }
    Ok(())
}

/// 验证工作区是否有效（存在 manifest 和 projects 目录）。
pub fn validate_workspace(path: &Path) -> Result<bool> {
    if path.join("workspace_manifest.json").exists() && path.join("projects").exists() {
        Ok(true)
    } else {
        Ok(false)
    }
}

use serde::{Deserialize, Serialize};

/// 最近编辑记录，用于首页展示"继续写作"入口。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RecentEdit {
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub timestamp: String,
}

use std::collections::HashMap;
use std::path::PathBuf;
/// 进程内缓存：避免频繁磁盘读取。
/// Key = workspace_path，Value = 最近编辑列表。
static RECENT_EDITS_CACHE: OnceLock<Mutex<HashMap<PathBuf, Vec<RecentEdit>>>> = OnceLock::new();

/// 获取最近编辑列表（优先从缓存读取，否则从磁盘加载）。
pub fn get_recent_edits(workspace_path: &Path) -> Result<Vec<RecentEdit>> {
    let mutex = RECENT_EDITS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = mutex.lock().unwrap();

    if let Some(edits) = cache.get(workspace_path) {
        return Ok(edits.to_vec());
    }

    let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
    if !recent_path.exists() {
        cache.insert(workspace_path.to_path_buf(), Vec::new());
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&recent_path)?;
    let edits: Vec<RecentEdit> = serde_json::from_str(&content).unwrap_or_default();

    cache.insert(workspace_path.to_path_buf(), edits.clone());
    Ok(edits)
}

/// 记录一次章节编辑（去重 + 插入头部 + 截断到 20 条 + 防抖落盘）。
///
/// 防抖策略：最多每 5 秒落盘一次，减少高频编辑时的 I/O 开销。
pub fn record_recent_edit(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) -> Result<()> {
    let mutex = RECENT_EDITS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = mutex.lock().unwrap();

    let mut edits = if let Some(e) = cache.get(workspace_path) {
        e.to_vec()
    } else {
        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
        if recent_path.exists() {
            let content = fs::read_to_string(&recent_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        }
    };

    // Remove existing entry for same chapter if exists
    edits.retain(|e| e.chapter_id != chapter_id);

    // Add new entry at the beginning
    edits.insert(
        0,
        RecentEdit {
            project_id: project_id.to_string(),
            volume_id: volume_id.to_string(),
            chapter_id: chapter_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        },
    );

    // Keep only top 20
    edits.truncate(20);

    cache.insert(workspace_path.to_path_buf(), edits.to_vec());

    // Basic Debounce: Only flush to disk at most once every 5 seconds to reduce I/O.
    static LAST_FLUSH: OnceLock<Mutex<std::time::Instant>> = OnceLock::new();
    let flush_mutex = LAST_FLUSH
        .get_or_init(|| Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(10)));
    let mut last_flush = flush_mutex.lock().unwrap();

    if last_flush.elapsed().as_secs() >= 5 {
        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
        let content = serde_json::to_string_pretty(&edits)?;
        crate::storage::atomic_write_string(&recent_path, &content)?;
        *last_flush = std::time::Instant::now();
    }

    Ok(())
}

/// 强制将 recent_edits 缓存落盘（用于应用退出、切换工作区等场景）。
pub fn flush_recent_edits(workspace_path: &Path) -> Result<()> {
    let mutex = RECENT_EDITS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cache = mutex.lock().unwrap();

    if let Some(edits) = cache.get(workspace_path) {
        let recent_path = workspace_path.join("app-meta/settings/recent_edits.json");
        let content = serde_json::to_string_pretty(edits)?;
        crate::storage::atomic_write_string(&recent_path, &content)?;
    }

    Ok(())
}
