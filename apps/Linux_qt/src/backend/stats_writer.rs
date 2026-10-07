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
// QObject/QML 对象。data root 变化时旧 writer 先 shutdown_and_join（drop sender
// 让 worker 消费完队列后退出，再 join 等待真正落盘），新 writer 用新 root 重建。
//
// Issue #843 复核评论 6045207375：两个生命周期问题修复。
// 问题1：spawn 返回的 JoinHandle 之前直接丢掉，drop Sender 只让 receiver.iter()
//         在消费完消息后退出，但没有等待 worker 真正退出，关程序时队列最后几条
//         统计事件可能来不及写盘。现在 handle 同时持有 sender 和 JoinHandle，
//         shutdown_and_join 先 drop sender 再 join worker，Drop 也调同一套。
// 问题2：写入在 worker thread，查询在 UI 线程用另一份 WriterCoreApi，两份 API
//         的内部锁不是同一把锁，对同一份 events.local/*.jsonl 没有顺序保证。
//         增加 Barrier 消息：查询前 UI 线程发 Barrier 并等 ack，worker 处理到
//         Barrier 时前面所有 Record 已落盘，回 ack，UI 线程再读磁盘。

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

/// worker 消息：写入命令或 barrier 同步点。
///
/// `Record` 携带一条写统计命令；`Barrier` 携带一个 ack sender，worker 处理到
/// 这条消息时说明前面所有 Record 已全部落盘，回 `ack.send(())` 让 UI 线程继续。
enum StatsWriterMessage {
    Record(StatsWriteCommand),
    Barrier(Sender<()>),
}

/// 进程内唯一的串行 stats writer handle。
///
/// UI 线程持有 sender（enqueue 立刻返回），独立 worker thread 按顺序消费。
/// worker 持有自己的 `WriterCoreApi`，不访问任何 QObject/QML 对象。
///
/// Issue #843 复核评论 6045207375：handle 同时持有 sender 和 worker JoinHandle。
/// `shutdown_and_join` 先 take/drop sender（让 receiver 在消费完队列后结束），
/// 再 join worker（等待尾部事件真正落盘）。`Drop` 也调同一套，保证即使忘记
/// 显式 shutdown 也不会丢统计。`sender`/`worker` 用 `Option` 是因为
/// `shutdown_and_join` 会 take 它们，Drop 时再次调用要能容忍已 take 的情况。
pub(crate) struct StatsWriterHandle {
    sender: Option<Sender<StatsWriterMessage>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl StatsWriterHandle {
    /// 创建新 writer：启动 worker thread，worker 持有自己的 WriterCoreApi。
    pub(crate) fn start(
        data_root: String,
        projects_root: String,
        layout: GitRepoLayout,
    ) -> Self {
        let (sender, receiver) = mpsc::channel::<StatsWriterMessage>();
        let worker = std::thread::Builder::new()
            .name("stats-writer".to_string())
            .spawn(move || {
                let api = crate::backend::app_backend::with_layout_core_api(
                    &data_root,
                    &projects_root,
                    &layout,
                );
                for msg in receiver.iter() {
                    match msg {
                        StatsWriterMessage::Record(cmd) => {
                            if let Err(e) = crate::writing_bridge::record_editor_change_stats(
                                &api,
                                &cmd.project_id,
                                &cmd.volume_id,
                                &cmd.chapter_id,
                                cmd.cause,
                                cmd.inserted_chars,
                                cmd.deleted_chars,
                            ) {
                                log::error!(
                                    "stats-writer: record_editor_change_stats failed: {}",
                                    e
                                );
                            }
                        }
                        StatsWriterMessage::Barrier(ack) => {
                            // 前面的 Record 已全部落盘，回 ack 让 UI 线程继续查询。
                            let _ = ack.send(());
                        }
                    }
                }
            })
            .expect("failed to spawn stats-writer thread");
        Self {
            sender: Some(sender),
            worker: Some(worker),
        }
    }

    /// UI 线程调用：把命令放进 channel，立刻返回。
    /// worker 已退出时 send 返回 Err，命令被丢弃——不影响 UI。
    pub(crate) fn enqueue(&self, cmd: StatsWriteCommand) {
        if let Some(ref sender) = self.sender {
            let _ = sender.send(StatsWriterMessage::Record(cmd));
        }
    }

    /// 同步 barrier：等 worker 把前面所有 Record 落盘后再返回。
    ///
    /// 只在查询统计时调用，不在输入热路径调用。worker 串行消费，处理到 Barrier
    /// 时前面所有 Record 已落盘，回 ack 后 UI 线程再读磁盘，保证查询看到最新数据。
    ///
    /// 返回 `Ok(())` 表示 barrier 成功（或没有 writer，直接通过）；
    /// 返回 `Err(())` 表示 worker 已退出（sender send 失败或 ack recv 失败），
    /// 调用方应走明确错误/空结果，不能拿旧磁盘数据冒充最新结果。
    pub(crate) fn barrier(&self) -> Result<(), ()> {
        if let Some(ref sender) = self.sender {
            let (ack_tx, ack_rx) = std::sync::mpsc::channel::<()>();
            if sender.send(StatsWriterMessage::Barrier(ack_tx)).is_err() {
                return Err(()); // worker 已退出
            }
            ack_rx.recv().map_err(|_| ())
        } else {
            Ok(()) // sender 已被 take（shutdown 过），按无 writer 处理
        }
    }

    /// 先 drop sender 让 worker 消费完队列后退出，再 join worker 等待尾部事件落盘。
    ///
    /// 幂等：多次调用安全，第二次 take 拿到 None 直接跳过。
    /// `reset_workspace_state` / `internal_open_data_root` 切换 workspace 时显式
    /// 调用，避免两个 writer 生命周期重叠或丢统计；`Drop` 也调同一套兜底。
    pub(crate) fn shutdown_and_join(&mut self) {
        // 先 take/drop sender，让 receiver 在消费完队列后结束。
        if let Some(sender) = self.sender.take() {
            drop(sender);
        }
        // 再 join worker，等待尾部事件落盘。
        if let Some(worker) = self.worker.take() {
            if let Err(e) = worker.join() {
                log::error!("stats-writer: worker thread join failed: {:?}", e);
            }
        }
    }
}

impl Drop for StatsWriterHandle {
    fn drop(&mut self) {
        // 兜底：即使上层忘记显式 shutdown_and_join，drop 时也等 worker 把尾部
        // 事件落盘，避免关程序时丢统计。
        self.shutdown_and_join();
    }
}
