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
//!
//! ## 章节归属校正（Issue #632）
//!
//! `recent_edits.json` 里历史遗留的 `(project_id, volume_id, chapter_id)` 三元组
//! 可能因为作品/卷/章节被移动、重建或导入而失效，也可能出现同一章节被记成
//! 两个不同 project_id 的"alias"。本模块在读取和写入时都按**当前作品树**
//! 重新解析章节真实归属：先做快速直检（`<projects_root>/<pid>/volumes/<vid>/chapters/<cid>/chapter.meta.json`），
//! 命中即采用；否则扫描当前作品树找到该 chapter_id 的真实 (project_id, volume_id)。
//! 找不到说明章节已不存在，recent 本就不该继续展示，直接丢弃。

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
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RecentEdit {
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub timestamp: String,
}

/// 进程内缓存 key。
///
/// 规范化依赖当前作品树，因此 key 必须同时包含 `app_data_root` 和
/// `projects_root`，避免不同作品树共用同一份缓存导致 alias 残留。
#[derive(Hash, Eq, PartialEq, Clone)]
pub(crate) struct RecentEditsCacheKey {
    pub(crate) app_data_root: PathBuf,
    pub(crate) projects_root: PathBuf,
}

type RecentEditsCache = HashMap<RecentEditsCacheKey, Vec<RecentEdit>>;

/// 进程内缓存：避免频繁磁盘读取。
/// Key = `RecentEditsCacheKey`，Value = 最近编辑列表。
///
/// 使用 `OnceLock<Mutex<...>>` 实现懒初始化的进程级单例。
/// Mutex poison 被转换为 `Error::Other`（见 `lock_recent_edits_cache`），
/// 不会因 panic 传播而中断业务流程。
/// 当前仅 GUI 线程访问，但 Mutex 保证未来多线程扩展的安全性。
#[cfg(test)]
pub(crate) static RECENT_EDITS_CACHE: OnceLock<Mutex<RecentEditsCache>> = OnceLock::new();

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

/// 章节在当前作品树中的真实归属。
struct ChapterOwner {
    project_id: String,
    volume_id: String,
}

/// 按"当前作品树"解析一条 recent edit 的真实章节归属。
///
/// 先做快速直检：若 `<projects_root>/<pid>/volumes/<vid>/chapters/<cid>/chapter.meta.json`
/// 存在，三元组仍然有效，直接采用。否则扫描当前作品树，找到该 `chapter_id`
/// 真正所属的 (project_id, volume_id)。找不到返回 `None`，表示章节已不存在。
fn resolve_current_chapter_owner(
    projects_root: &Path,
    edit: &RecentEdit,
) -> Result<Option<ChapterOwner>> {
    let direct = projects_root
        .join(&edit.project_id)
        .join("volumes")
        .join(&edit.volume_id)
        .join("chapters")
        .join(&edit.chapter_id)
        .join("chapter.meta.json");

    if direct.is_file() {
        return Ok(Some(ChapterOwner {
            project_id: edit.project_id.clone(),
            volume_id: edit.volume_id.clone(),
        }));
    }

    for project in crate::project::list_projects(projects_root)? {
        let project_root = projects_root.join(&project.id);
        if let Some(volume_id) = find_volume_containing_chapter(&project_root, &edit.chapter_id)? {
            return Ok(Some(ChapterOwner {
                project_id: project.id,
                volume_id,
            }));
        }
    }

    Ok(None)
}

/// 在单个作品下查找包含指定 chapter_id 的 volume_id。
fn find_volume_containing_chapter(project_root: &Path, chapter_id: &str) -> Result<Option<String>> {
    for volume in crate::volume::list_volumes(project_root)? {
        let chapters = crate::chapter::list_chapters(project_root, &volume.id)?;
        if chapters.iter().any(|c| c.id == chapter_id) {
            return Ok(Some(volume.id));
        }
    }
    Ok(None)
}

/// 获取最近编辑列表（优先从缓存读取，否则从磁盘加载）。
///
/// 不管来源是磁盘还是进程缓存，都先对当前作品树重新规范化；
/// 规范化结果和磁盘原始列表不同时，用 `atomic_write_string()` 把
/// `recent_edits.json` 真正改干净，避免下次启动旧 alias 又回来。
pub fn get_recent_edits(app_data_root: &Path, projects_root: &Path) -> Result<Vec<RecentEdit>> {
    let mutex = RECENT_EDITS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = lock_recent_edits_cache(mutex)?;
    let key = RecentEditsCacheKey {
        app_data_root: app_data_root.to_path_buf(),
        projects_root: projects_root.to_path_buf(),
    };

    if let Some(edits) = cache.get(&key) {
        return Ok(edits.to_vec());
    }

    let recent_path = app_data_root.join("recent_edits.json");
    let raw: Vec<RecentEdit> = if recent_path.exists() {
        let content = fs::read_to_string(&recent_path)?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        Vec::new()
    };

    let edits = normalize_recent_edits(projects_root, raw.clone())?;

    // 规范化结果与磁盘原始列表不同（含重复 alias、失效条目、未排序等）时，
    // 用原子写把磁盘真正改干净，避免下次启动旧 alias 又回来。
    if edits != raw {
        let content = serde_json::to_string_pretty(&edits)?;
        crate::storage::atomic_write_string(&recent_path, &content)?;
    }

    cache.insert(key, edits.clone());
    Ok(edits)
}

/// 计算一次章节编辑（去重 + 插入头部 + 截断到 MAX_RECENT_EDITS 条 + 防抖落盘）。
///
/// 防抖策略：最多每 RECENT_EDITS_FLUSH_INTERVAL（5 秒）落盘一次，
/// 减少高频编辑时的 I/O 开销。应用退出时应调用
/// `flush_recent_edits` 强制落盘，防止丢失最近一条记录。
/// 注意：防抖期间进程崩溃可能丢失最后一次落盘之后的所有编辑记录。
///
/// 不直接相信调用方传来的三元组：先走 `resolve_current_chapter_owner`
/// 得到 canonical `project_id/volume_id` 后再插入，然后再按 canonical
/// project id 去重。这样"写入"和"读取"不会继续存在两套身份语义。
pub fn record_recent_edit(
    app_data_root: &Path,
    projects_root: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) -> Result<()> {
    let mutex = RECENT_EDITS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = lock_recent_edits_cache(mutex)?;
    let key = RecentEditsCacheKey {
        app_data_root: app_data_root.to_path_buf(),
        projects_root: projects_root.to_path_buf(),
    };

    let mut edits = if let Some(e) = cache.get(&key) {
        e.to_vec()
    } else {
        let recent_path = app_data_root.join("recent_edits.json");
        if recent_path.exists() {
            let content = fs::read_to_string(&recent_path)?;
            let raw: Vec<RecentEdit> = serde_json::from_str(&content).unwrap_or_default();
            // #630 评论12 项1：磁盘数据进入缓存前统一走 normalize，
            // 确保旧数据里的重复项不会绕过去重进入缓存。
            // #632：normalize 现在按当前作品树校正章节归属。
            normalize_recent_edits(projects_root, raw)?
        } else {
            Vec::new()
        }
    };

    // #632：不直接相信调用方传来的三元组，先解析 canonical owner。
    // 解析失败（章节尚未落盘等）时退回原始三元组，避免刚编辑的章节丢失入口。
    let candidate = RecentEdit {
        project_id: project_id.to_string(),
        volume_id: volume_id.to_string(),
        chapter_id: chapter_id.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    let canonical = match resolve_current_chapter_owner(projects_root, &candidate)? {
        Some(owner) => RecentEdit {
            project_id: owner.project_id,
            volume_id: owner.volume_id,
            chapter_id: candidate.chapter_id,
            timestamp: candidate.timestamp,
        },
        None => candidate,
    };

    // Remove existing entry for same canonical project if exists
    edits.retain(|e| e.project_id != canonical.project_id);

    // Add new entry at the beginning
    edits.insert(0, canonical);

    // Keep only top MAX_RECENT_EDITS
    edits.truncate(MAX_RECENT_EDITS);

    cache.insert(key, edits.clone());

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
///
/// `projects_root` 仅用于和 get/record 共享同一份缓存 key，flush 本身
/// 不做规范化。
pub fn flush_recent_edits(app_data_root: &Path, projects_root: &Path) -> Result<()> {
    let mutex = RECENT_EDITS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cache = lock_recent_edits_cache(mutex)?;
    let key = RecentEditsCacheKey {
        app_data_root: app_data_root.to_path_buf(),
        projects_root: projects_root.to_path_buf(),
    };

    if let Some(edits) = cache.get(&key) {
        let recent_path = app_data_root.join("recent_edits.json");
        let content = serde_json::to_string_pretty(edits)?;
        crate::storage::atomic_write_string(&recent_path, &content)?;
    }

    Ok(())
}

/// 对最近编辑列表按**当前作品树**校正并去重：
///
/// 1. 对每条 edit 解析其章节在当前作品树中的真实 (project_id, volume_id)；
///    找不到说明章节已不存在，直接丢弃（recent 本就不该继续展示）。
/// 2. 用校正后的 project_id 作为身份按 project 只保留时间最新的一条。
/// 3. 按 timestamp 降序排序，截断到 `MAX_RECENT_EDITS` 条。
///
/// 注意：身份是 project_id（校正后），不是作品标题——标题不是身份。
fn normalize_recent_edits(projects_root: &Path, edits: Vec<RecentEdit>) -> Result<Vec<RecentEdit>> {
    let mut best = HashMap::<String, RecentEdit>::new();

    for mut edit in edits {
        let Some(owner) = resolve_current_chapter_owner(projects_root, &edit)? else {
            continue; // 章节已经不存在，recent 本来就不该继续展示
        };

        edit.project_id = owner.project_id;
        edit.volume_id = owner.volume_id;

        match best.get(&edit.project_id) {
            Some(existing) if existing.timestamp >= edit.timestamp => {}
            _ => {
                best.insert(edit.project_id.clone(), edit);
            }
        }
    }

    let mut result: Vec<_> = best.into_values().collect();
    // 按 timestamp 降序排序（最新的在前）
    result.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    result.truncate(MAX_RECENT_EDITS);
    Ok(result)
}

#[cfg(test)]
mod tests;
