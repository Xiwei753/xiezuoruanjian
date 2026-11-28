use std::path::Path;

use super::extractor;
use crate::error::Result;

/// 从磁盘提取一次搜索索引。
///
/// 返回成功提取的条目和失败来源：只有顶层目录无法枚举等整体失败才返回 `Err`；
/// 单个作品/卷/章节/星图/设置文件的读取或解析失败记录在
/// [`extractor::ExtractionReport`] 的 `failures` 中，由索引更新入口保留对应旧条目。
pub fn rebuild_index(
    app_data_root: &Path,
    projects_root: &Path,
    project_id: Option<&str>,
) -> Result<extractor::ExtractionReport> {
    let mut report = extractor::extract_chapter_entries(projects_root, project_id)?;
    report.extend(extractor::extract_starmap_entries(
        app_data_root,
        project_id,
    )?);
    report.extend(extractor::extract_setting_entries(app_data_root)?);
    Ok(report)
}
