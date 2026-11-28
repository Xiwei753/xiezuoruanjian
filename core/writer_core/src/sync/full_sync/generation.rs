//! 旧 `__generations__` 远端布局的只读兼容 helpers。
//!
//! 新同步只用远端固定 HEAD + 对象索引 delta（Issue #1066）；这里的常量与
//! 路径判断只用于读取/迁移旧仓库，以及清理时跳过历史 generation 目录。

use std::path::Path;

/// generation 原子发布 — 不可见 generation prefix 的子目录名。
///
/// LiveProject 先把完整 Project 上传到 `projects/P/__generations__/G/`，
/// CAS `targets.sync.json` 成功后才成为可见版本。`delete_all_remote_objects`
/// 跳过此子目录，不碰并发 Upsert 正在上传的 generation。
pub(super) const GENERATION_SUBDIR: &str = "__generations__";

/// 构造 generation 不可见 prefix。
///
/// `projects/P` + `G` → `projects/P/__generations__/G`。
///
///   防御性校验 `generation_id` 是合法单 path segment，
/// 不只依赖 catalog loader。非法 `generation_id`（空、`.`、`..`、含 `/`/`\`）→ `Err`。
pub(super) fn generation_remote_prefix(
    project_remote_prefix: &str,
    generation_id: &str,
) -> crate::error::Result<String> {
    crate::sync::target_lifecycle::validate_generation_id(generation_id)?;
    Ok(format!(
        "{}/{}/{}",
        project_remote_prefix, GENERATION_SUBDIR, generation_id
    ))
}

/// 判断远端相对路径是否落在 generation 不可见 prefix 下。
///
/// `delete_all_remote_objects` 用此跳过 `__generations__/` 下的对象，不碰并发
/// Upsert 正在上传的 generation。
pub(super) fn is_generation_path(rel_path: &str) -> bool {
    rel_path == GENERATION_SUBDIR || rel_path.starts_with(&format!("{}/", GENERATION_SUBDIR))
}

/// 把单个远端对象内容写入 staging（创建父目录 + 写文件）。
///
/// staging 条目可能与 live 共享 inode（seed 用硬链接），因此必须先 unlink
/// 再新建：就地截断会把 live 内容一起改掉。
pub(super) fn write_staging_file(staging: &Path, rel: &str, content: &[u8]) -> std::io::Result<()> {
    let dest = staging.join(rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::symlink_metadata(&dest) {
        Ok(_) => std::fs::remove_file(&dest)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::write(&dest, content)
}
