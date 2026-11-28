//! 远端作品发现 — 只用普通对象判断"远端还有这个作品"。
//!
//! 作品不是独立的生命周期对象：远端只要还有一个非同步元数据对象落在
//! `projects/<id>/` 下，这个作品就还存在。全部对象被删除后，前缀里只剩
//! `app-meta/sync/`（对象索引），不再算作作品，直到有新的对象写入。

use std::collections::BTreeSet;

use crate::sync::provider::SyncProvider;

/// 同步引擎自己的元数据目录；它描述对象，不代表作品还有内容。
const SYNC_META_DIR: &str = "app-meta/";

/// 列出远端还存在的作品 id。
///
/// 失败返回错误：网络错误不能被当成"远端没有作品"，否则下一次同步会把
/// 本机所有作品当成新作品重新上传。
pub(crate) fn discover_remote_project_ids(
    provider: &dyn SyncProvider,
) -> crate::error::Result<Vec<String>> {
    let entries = provider.list("projects")?;
    let mut with_content = BTreeSet::new();
    for entry in entries {
        let path = entry.path.trim_start_matches('/');
        let Some((project_id, relative)) = path.split_once('/') else {
            continue;
        };
        if project_id.is_empty()
            || project_id.starts_with('.')
            || project_id.starts_with('_')
            || relative.is_empty()
            || relative.starts_with(SYNC_META_DIR)
            || relative.starts_with("__generations__/")
            || relative.contains("/__generations__/")
        {
            continue;
        }
        with_content.insert(project_id.to_string());
    }
    Ok(with_content.into_iter().collect())
}
