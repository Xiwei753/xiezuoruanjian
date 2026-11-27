// =============================================================================
// chapter_draft/store/text.rs — 草稿正文的不可变 blob 存储
// =============================================================================
//
// Issue #1062 评论 7194373427 第 2 项：正文不能再和 meta 各写一次“当前版本”。
// 每次发布正文都写成 `texts/<revision>-<digest>.txt` 的不可变 blob，`meta.json`
// 只保存它引用的 blob 文件名与摘要；引用切换是唯一一次原子写。这样崩溃后
// 永远只有两种状态：旧 meta → 旧 blob，或新 meta → 新 blob，不存在
// “旧 meta 校验新正文”的版本对。

use std::path::{Path, PathBuf};

use super::{DraftLocation, MetaData, TEXTS_DIR, TEXT_FILE};

fn texts_dir(dir: &Path) -> PathBuf {
    dir.join(TEXTS_DIR)
}

fn digest_file_part(digest: &str) -> String {
    if digest.is_empty() {
        return "no-digest".to_string();
    }
    digest
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// 正文 blob 的稳定文件名：同一 revision+摘要永远映射到同一文件。
pub(crate) fn blob_name(revision: u64, digest: &str) -> String {
    format!("{revision:016}-{}.txt", digest_file_part(digest))
}

pub(crate) fn blob_path(dir: &Path, name: &str) -> PathBuf {
    texts_dir(dir).join(name)
}

/// 先写不可变正文 blob，返回文件名；meta 安装成功前它只是孤儿文件。
pub(crate) fn write_text_blob(
    dir: &Path,
    revision: u64,
    digest: &str,
    text: &str,
) -> Result<String, String> {
    let name = blob_name(revision, digest);
    let path = blob_path(dir, &name);
    writer_core::storage::atomic_write_string(&path, text)
        .map_err(|error| format!("write chapter draft text blob {}: {error}", path.display()))?;
    Ok(name)
}

fn read_blob(dir: &Path, name: &str) -> Result<Option<String>, String> {
    let path = blob_path(dir, name);
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "read chapter draft text blob {}: {error}",
            path.display()
        )),
    }
}

fn read_legacy_text(location: &DraftLocation) -> Option<String> {
    let path = location.dir.join(TEXT_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            log::warn!(
                "read chapter draft text failed: project={} chapter={} path={}: {error}",
                location.project_id,
                location.chapter_id,
                path.display()
            );
            None
        }
    }
}

/// meta 当前引用的正文：不可变 blob 优先，其次旧格式 `text.txt`。
///
/// meta 指向的 blob 缺失是未决损坏，必须报错保留目录，不能降级成“没有正文”。
pub(crate) fn read_referenced_text(
    location: &DraftLocation,
    meta: &MetaData,
) -> Result<Option<String>, String> {
    if meta.text_blob.is_empty() {
        return Ok(read_legacy_text(location));
    }
    match read_blob(&location.dir, &meta.text_blob)? {
        Some(text) => Ok(Some(text)),
        None => Err(format!(
            "chapter draft text blob missing (kept): project={} chapter={} blob={}",
            location.project_id, location.chapter_id, meta.text_blob
        )),
    }
}

/// 回收未被 meta 引用的正文 blob；旧格式 `text.txt` 在 meta 已指向 blob 后
/// 同样视为过期。
pub(crate) fn gc_text_blobs(dir: &Path, keep: Option<&str>) -> Result<usize, String> {
    let mut removed = 0usize;
    let texts = texts_dir(dir);
    match std::fs::read_dir(&texts) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|error| {
                    format!("read chapter draft texts dir {}: {error}", texts.display())
                })?;
                let path = entry.path();
                if keep.is_some_and(|keep| {
                    path.file_name().and_then(|name| name.to_str()) == Some(keep)
                }) {
                    continue;
                }
                if path.is_file() {
                    match std::fs::remove_file(&path) {
                        Ok(()) => removed = removed.saturating_add(1),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            return Err(format!(
                                "remove chapter draft text blob {}: {error}",
                                path.display()
                            ))
                        }
                    }
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("read draft texts dir {}: {error}", texts.display())),
    }
    let legacy = dir.join(TEXT_FILE);
    if legacy.exists() {
        match std::fs::remove_file(&legacy) {
            Ok(()) => removed = removed.saturating_add(1),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "remove legacy chapter draft text {}: {error}",
                    legacy.display()
                ))
            }
        }
    }
    Ok(removed)
}

/// 当前草稿指向的正文内容（测试/诊断用；不改变磁盘）。
#[cfg(test)]
pub(crate) fn current_text(location: &DraftLocation) -> Option<String> {
    let meta =
        super::read_meta_checked(&location.dir, &location.project_id, &location.chapter_id).ok()?;
    read_referenced_text(location, &meta).ok().flatten()
}

/// 当前草稿正文所在文件（测试/诊断用）：blob 或旧 `text.txt`。
#[cfg(test)]
pub(crate) fn current_text_path(location: &DraftLocation) -> Option<PathBuf> {
    let meta =
        super::read_meta_checked(&location.dir, &location.project_id, &location.chapter_id).ok()?;
    if meta.text_blob.is_empty() {
        let legacy = location.dir.join(TEXT_FILE);
        return legacy.exists().then_some(legacy);
    }
    let path = blob_path(&location.dir, &meta.text_blob);
    path.exists().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_name_is_stable_and_filesystem_safe() {
        assert_eq!(
            blob_name(7, "content-root-v1:ab/cd"),
            "0000000000000007-content-root-v1_ab_cd.txt"
        );
        assert_eq!(blob_name(0, ""), "0000000000000000-no-digest.txt");
        assert_eq!(blob_name(12, "abc"), blob_name(12, "abc"));
    }
}
