use std::sync::atomic::{AtomicU64, Ordering};

/// 全局布局修订号生成器 — 进程内唯一，跨所有 LinuxEditorPipeline 实例。
///
/// 使用 Relaxed ordering：LayoutRevision 仅用于标识"布局是否变化"的快速比较，
/// 不承担同步语义。同一 Pipeline 内的布局重算顺序由调用方保证。
static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

/// 布局修订号 — 标识一次布局快照的唯一版本。
///
/// 与 EditorKernel 的 text_revision 不同：text_revision 跟踪文本内容变更，
/// LayoutRevision 跟踪排版结果变更（含文本变更触发的重排、窗口宽度变化、
/// 字号变化等）。两者独立递增。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LayoutRevision(pub u64);

impl LayoutRevision {
    pub fn next() -> Self {
        LayoutRevision(NEXT_REVISION.fetch_add(1, Ordering::Relaxed))
    }

    pub fn initial() -> Self {
        LayoutRevision(0)
    }
}

impl Default for LayoutRevision {
    fn default() -> Self {
        Self::initial()
    }
}
