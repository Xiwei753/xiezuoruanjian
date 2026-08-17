//! # 最近编辑记录（Core 层）
//!
//! 负责记录和查询最近编辑的章节，用于首页展示"继续写作"入口。
//!
//! ## 职责边界
//!
//! - **做**：记录最近编辑、查询最近编辑列表、防抖落盘
//! - **不做**：项目内容管理、同步逻辑、设置管理（由其他模块负责）
//!
//! ## 存储位置
//!
//! ```text
//! app_data_root/
//!   recent_edits.json    # 最近编辑列表
//! ```

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

/// 最近编辑列表最大条数。
const MAX_RECENT_EDITS: usize = 20;

/// 最近编辑缓存落盘防抖间隔。
const RECENT_EDITS_FLUSH_INTERVAL: Duration = Duration::from_secs(5);

/// 最近编辑记录，用于首页展示"继续写作"入口。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RecentEdit {
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub timestamp: String,
}

type RecentEditsCache = HashMap<PathBuf, Vec<RecentEdit>>;

/// 进程内缓存：避免频繁磁盘读取。
/// Key = app_data_root，Value = 最近编辑列表。
///
/// 使用 `OnceLock<Mutex<...>>` 实现懒初始化的进程级单例。
/// Mutex poison 被转换为 `Error::Other`（见 `lock_recent_edits_cache`），
/// 不会因 panic 传播而中断业务流程。
/// 当前仅 GUI 线程访问，但 Mutex 保证未来多线程扩展的安全性。
#[cfg(test)]
pub static RECENT_EDITS_CACHE: OnceLock<Mutex<RecentEditsCache>> = OnceLock::new();

#[cfg(not(test))]
static RECENT_EDITS_CACHE: OnceLock<Mutex<RecentEditsCache>> = OnceLock::new();

/// 锁定 recent_edits 缓存，返回 MutexGuard。
///
/// 将 Mutex poison 转为 `Error::Other`，避免生产代码因 panic 中断。
fn lock_recent_edits_cache(
    mutex: &Mutex<RecentEditsCache>,
) -> Result<MutexGuard<'_, RecentEditsCache>> {
    mutex
        .lock()
        .map_err(|_| crate::error::Error::Other("recent edits cache mutex poisoned".into()))
}

/// 锁定 LAST_FLUSH 计时器，返回 MutexGuard。
fn lock_last_flush(
    mutex: &Mutex<std::time::Instant>,
) -> Result<MutexGuard<'_, std::time::Instant>> {
    mutex
        .lock()
        .map_err(|_| crate::error::Error::Other("recent edits flush timer mutex poisoned".into()))
}

/// 获取最近编辑列表（优先从缓存读取，否则从磁盘加载）。
pub fn get_recent_edits(app_data_root: &Path) -> Result<Vec<RecentEdit>> {
    let mutex = RECENT_EDITS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = lock_recent_edits_cache(mutex)?;

    if let Some(edits) = cache.get(app_data_root) {
        return Ok(edits.to_vec());
    }

    let recent_path = app_data_root.join("recent_edits.json");
    if !recent_path.exists() {
        cache.insert(app_data_root.to_path_buf(), Vec::new());
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&recent_path)?;
    let edits: Vec<RecentEdit> = serde_json::from_str(&content).unwrap_or_default();
    let edits = normalize_recent_edits(edits);

    cache.insert(app_data_root.to_path_buf(), edits.clone());
    Ok(edits)
}

/// 计算一次章节编辑（去重 + 插入头部 + 截断到 MAX_RECENT_EDITS 条 + 防抖落盘）。
///
/// 防抖策略：最多每 RECENT_EDITS_FLUSH_INTERVAL（5 秒）落盘一次，
/// 减少高频编辑时的 I/O 开销。应用退出时应调用
/// `flush_recent_edits` 强制落盘，防止丢失最近一条记录。
/// 注意：防抖期间进程崩溃可能丢失最后一次落盘之后的所有编辑记录。
pub fn record_recent_edit(
    app_data_root: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) -> Result<()> {
    let mutex = RECENT_EDITS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = lock_recent_edits_cache(mutex)?;

    let mut edits = if let Some(e) = cache.get(app_data_root) {
        e.to_vec()
    } else {
        let recent_path = app_data_root.join("recent_edits.json");
        if recent_path.exists() {
            let content = fs::read_to_string(&recent_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        }
    };

    // Remove existing entry for same project if exists
    edits.retain(|e| e.project_id != project_id);

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

    // Keep only top MAX_RECENT_EDITS
    edits.truncate(MAX_RECENT_EDITS);

    cache.insert(app_data_root.to_path_buf(), edits.clone());

    // Debounce: only flush to disk at most once every RECENT_EDITS_FLUSH_INTERVAL.
    static LAST_FLUSH: OnceLock<Mutex<std::time::Instant>> = OnceLock::new();
    let flush_mutex = LAST_FLUSH
        .get_or_init(|| Mutex::new(std::time::Instant::now() - RECENT_EDITS_FLUSH_INTERVAL * 2));
    let mut last_flush = lock_last_flush(flush_mutex)?;

    if last_flush.elapsed() >= RECENT_EDITS_FLUSH_INTERVAL {
        let recent_path = app_data_root.join("recent_edits.json");
        let content = serde_json::to_string_pretty(&edits)?;
        crate::storage::atomic_write_string(&recent_path, &content)?;
        *last_flush = std::time::Instant::now();
    }

    Ok(())
}

/// 强制将 recent_edits 缓存落盘（用于应用退出等场景）。
pub fn flush_recent_edits(app_data_root: &Path) -> Result<()> {
    let mutex = RECENT_EDITS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cache = lock_recent_edits_cache(mutex)?;

    if let Some(edits) = cache.get(app_data_root) {
        let recent_path = app_data_root.join("recent_edits.json");
        let content = serde_json::to_string_pretty(edits)?;
        crate::storage::atomic_write_string(&recent_path, &content)?;
    }

    Ok(())
}

/// 对最近编辑列表去重：按 project_id 只保留时间最新的一条，然后截断。
/// 用于磁盘加载旧数据时规范化，确保 MAX_RECENT_EDITS 的含义是"最近作品数"。
fn normalize_recent_edits(edits: Vec<RecentEdit>) -> Vec<RecentEdit> {
    let mut best: HashMap<String, RecentEdit> = HashMap::new();
    for edit in edits {
        match best.get(&edit.project_id) {
            Some(existing) if existing.timestamp >= edit.timestamp => {}
            _ => {
                best.insert(edit.project_id.clone(), edit);
            }
        }
    }
    let mut result: Vec<RecentEdit> = best.into_values().collect();
    // 按 timestamp 降序排序（最新的在前）
    result.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    result.truncate(MAX_RECENT_EDITS);
    result
}

#[cfg(test)]
mod tests;
