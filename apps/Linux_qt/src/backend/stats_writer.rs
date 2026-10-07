// =============================================================================
// stats_writer.rs — 异步串行统计写入器，避免统计 I/O 阻塞 UI 线程
// =============================================================================
//
// Issue #843: Linux_Qt 没有 Android 的 IO actor，StatsStore::record_event()
// 每条事件都做 create_dir_all -> open -> writeln -> close，每次按键都可能把
// 文件系统延迟塞回 UI 主线程。
//
// 解法：进程内唯一的串行 stats writer。UI 线程持有 sender（enqueue 立刻返回），
// 独立 worker thread 按顺序消费。worker 持有自己的 WriterCoreApi，不访问任何
// QObject/QML 对象。data root 变化时旧 writer drop（sender 关闭，worker 自然
// 退出），新 writer 用新 root 重建。

use std::sync::mpsc::{self, Sender};
use writer_core::editor::EditorTransactionCause;
use writer_core::storage::git_repo_layout::GitRepoLayout;

/// UI 线程构造的写统计命令。只带编辑事实，不带 QObject/QML 引用。
pub(crate) struct StatsWriteCommand {
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub cause: EditorTransactionCause,
    pub inserted_chars: u32,
    pub deleted_chars: u32,
}

/// 进程内唯一的串行 stats writer handle。
///
/// UI 线程持有 sender（enqueue 立刻返回），独立 worker thread 按顺序消费。
/// worker 持有自己的 `WriterCoreApi`，不访问任何 QObject/QML 对象。
/// data root 变化时旧 writer drop（sender 关闭，worker 自然退出），新 writer 用新 root 重建。
pub(crate) struct StatsWriterHandle {
    sender: Sender<StatsWriteCommand>,
}

impl StatsWriterHandle {
    /// 创建新 writer：启动 worker thread，worker 持有自己的 WriterCoreApi。
    pub(crate) fn start(
        data_root: String,
        projects_root: String,
        layout: GitRepoLayout,
    ) -> Self {
        let (sender, receiver) = mpsc::channel::<StatsWriteCommand>();
        std::thread::Builder::new()
            .name("stats-writer".to_string())
            .spawn(move || {
                let api = crate::backend::app_backend::with_layout_core_api(
                    &data_root,
                    &projects_root,
                    &layout,
                );
                for cmd in receiver.iter() {
                    if let Err(e) = crate::writing_bridge::record_editor_change_stats(
                        &api,
                        &cmd.project_id,
                        &cmd.volume_id,
                        &cmd.chapter_id,
                        cmd.cause,
                        cmd.inserted_chars,
                        cmd.deleted_chars,
                    ) {
                        log::error!("stats-writer: record_editor_change_stats failed: {}", e);
                    }
                }
            })
            .expect("failed to spawn stats-writer thread");
        Self { sender }
    }

    /// UI 线程调用：把命令放进 channel，立刻返回。
    /// worker 已退出时 send 返回 Err，命令被丢弃——不影响 UI。
    pub(crate) fn enqueue(&self, cmd: StatsWriteCommand) {
        let _ = self.sender.send(cmd);
    }
}
