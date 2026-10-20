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
//
// Issue #935：QML 统计页的一次性查询不再走「UI 线程 barrier().recv() 再读盘」，
// 而是把 QueryDashboard / QueryWritingStatus 排进同一条串行队列。worker 消费到
// 查询时，此前全部 Record 已处理完，它在 worker thread 里一次性生成快照/状态并
// 通过 completion 回报；completion 由上层用 queued_callback 投递回 GUI 线程。
// 队列消费到查询时若前面有 Record 写盘失败，先向下一次查询回报错误，不假装成功。
//
// Issue #935 复核：写盘失败不能只回报一次就假装成功。失败命令保留在 worker 的
// 待补写列表里，用首次入队就固定的 event_id / occurred_at_ms 在同一串行队列中
// 重放；查询前先补写，仍有失败则回报错误而不是表面成功的快照。

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
    /// Issue #935 复核：编辑事实首次入队就固定稳定身份与发生时间，
    /// 写盘失败重发时原样带回，避免每次重发都被 Core 当成新事件。
    pub event_id: String,
    pub occurred_at_ms: i64,
}

/// 统计查询完成回调：worker 在 worker thread 上调用，交回 `(request_id, 结果)`。
///
/// 实现方负责把结果投递回 GUI 线程（上层用 `qmetaobject::queued_callback` 包装），
/// 这里不接触任何 QObject/QML 对象。
pub(crate) type StatsQueryCallback = Box<dyn FnOnce(u32, Result<String, String>) + Send + Sync>;

/// worker 消息：写入命令、barrier 同步点或一次性查询。
///
/// `Record` 携带一条写统计命令；`Barrier` 携带一个 ack sender，worker 处理到
/// 这条消息时说明前面所有 Record 已全部落盘，回 `ack.send(())` 让 UI 线程继续。
/// `QueryDashboard` / `QueryWritingStatus`（Issue #935）与 Record 走同一条队列，
/// 消费到查询时此前全部编辑事实已处理完，回调带回最新快照或真实错误。
enum StatsWriterMessage {
    Record(StatsWriteCommand),
    Barrier(Sender<()>),
    QueryDashboard {
        request_id: u32,
        completion: StatsQueryCallback,
    },
    QueryWritingStatus {
        request_id: u32,
        window_seconds: u32,
        completion: StatsQueryCallback,
    },
    /// Issue #946：统计页 Explorer 快照（生涯摘要 + 四个时段卡 + 选中时段详情）。
    ///
    /// `preset` 为固定字符串（`last7Days` / `last30Days` / `last6Months` /
    /// `lifetime` / `custom`）；`custom` 必须同时携带 `start` / `end`
    /// （`YYYY-MM-DD`，两端包含），其余预设两端为 `None`。
    QueryStatsExplorer {
        request_id: u32,
        preset: String,
        start: Option<String>,
        end: Option<String>,
        completion: StatsQueryCallback,
    },
    /// Issue #960：统计页每日码字日历（指定公历年的逐日人工输入快照）。
    ///
    /// 与 Explorer 同队列：worker 消费到它时此前全部 Record 已落盘，
    /// 直接复用 Core 的 `get_writing_stats_heatmap_json`，不在 Qt 端重算每日字数。
    QueryStatsCalendar {
        request_id: u32,
        year: i32,
        completion: StatsQueryCallback,
    },
}

/// 把一条编辑事实写进 Core 统计；失败时记录日志并保留原始命令供补写。
fn try_record_editor_change_stats(
    api: &writer_core::api::WriterCoreApi,
    cmd: &StatsWriteCommand,
) -> Result<(), String> {
    crate::writing_bridge::record_editor_change_stats(
        api,
        &cmd.project_id,
        &cmd.volume_id,
        &cmd.chapter_id,
        cmd.cause,
        cmd.inserted_chars,
        cmd.deleted_chars,
        &cmd.event_id,
        cmd.occurred_at_ms,
    )
    .map_err(|e| {
        log::error!(
            "stats-writer: record_editor_change_stats failed (event_id={}): {e}",
            cmd.event_id
        );
        format!("record_editor_change_stats: {e}")
    })
}

/// 按入队顺序重放待补写命令，返回仍未成功的数量。
fn flush_pending_records(
    api: &writer_core::api::WriterCoreApi,
    pending_records: &mut Vec<StatsWriteCommand>,
) -> usize {
    if pending_records.is_empty() {
        return 0;
    }
    let mut still_failed = Vec::new();
    for cmd in pending_records.drain(..) {
        if try_record_editor_change_stats(api, &cmd).is_err() {
            still_failed.push(cmd);
        }
    }
    *pending_records = still_failed;
    pending_records.len()
}

/// 先补写再判断：全部写盘成功返回 None，否则返回给查询的错误。
fn pending_records_failure(
    api: &writer_core::api::WriterCoreApi,
    pending_records: &mut Vec<StatsWriteCommand>,
) -> Option<String> {
    let remaining = flush_pending_records(api, pending_records);
    if remaining == 0 {
        None
    } else {
        Some(format!(
            "stats-writer: {remaining} 条编辑事实尚未写盘成功，暂不返回统计结果"
        ))
    }
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
    pub(crate) fn start(data_root: String, projects_root: String, layout: GitRepoLayout) -> Self {
        let (sender, receiver) = mpsc::channel::<StatsWriterMessage>();
        let worker = std::thread::Builder::new()
            .name("stats-writer".to_string())
            .spawn(move || {
                let api = crate::backend::app_backend::with_layout_core_api(
                    &data_root,
                    &projects_root,
                    &layout,
                );
                // Issue #935 复核：Record 写盘失败不吞掉——失败命令保留在待补写
                // 列表里，用固定的 event_id / occurred_at_ms 在队列内重放；
                // 补写成功前，查询回报错误而不是表面成功。
                let mut pending_records: Vec<StatsWriteCommand> = Vec::new();
                for msg in receiver.iter() {
                    match msg {
                        StatsWriterMessage::Record(cmd) => {
                            if try_record_editor_change_stats(&api, &cmd).is_err() {
                                pending_records.push(cmd);
                            }
                        }
                        StatsWriterMessage::Barrier(ack) => {
                            // 先尽量补写前面的失败事实，再放行查询；仍失败的
                            // 留给 QueryDashboard/QueryWritingStatus 回报。
                            let _ = flush_pending_records(&api, &mut pending_records);
                            let _ = ack.send(());
                        }
                        StatsWriterMessage::QueryDashboard {
                            request_id,
                            completion,
                        } => {
                            if let Some(err) = pending_records_failure(&api, &mut pending_records) {
                                completion(request_id, Err(err));
                            } else {
                                completion(
                                    request_id,
                                    api.get_writing_stats_dashboard_json()
                                        .map_err(|e| e.to_string()),
                                );
                            }
                        }
                        StatsWriterMessage::QueryWritingStatus {
                            request_id,
                            window_seconds,
                            completion,
                        } => {
                            if let Some(err) = pending_records_failure(&api, &mut pending_records) {
                                completion(request_id, Err(err));
                            } else {
                                let result = api
                                    .get_current_writing_speed_json(window_seconds)
                                    .and_then(|speed| {
                                        api.get_today_writing_stats_summary_json()
                                            .map(|today| (speed, today))
                                    })
                                    .map(|(speed, today)| {
                                        format!("{{\"currentSpeed\":{speed},\"today\":{today}}}")
                                    })
                                    .map_err(|e| e.to_string());
                                completion(request_id, result);
                            }
                        }
                        StatsWriterMessage::QueryStatsExplorer {
                            request_id,
                            preset,
                            start,
                            end,
                            completion,
                        } => {
                            // Issue #946：与 QueryDashboard 同口径——先补写尚未落盘的
                            // 编辑事实，再在 worker thread 里生成 Explorer 快照。
                            if let Some(err) = pending_records_failure(&api, &mut pending_records) {
                                completion(request_id, Err(err));
                            } else {
                                completion(
                                    request_id,
                                    api.get_writing_stats_explorer_json(
                                        &preset,
                                        start.as_deref(),
                                        end.as_deref(),
                                    )
                                    .map_err(|e| e.to_string()),
                                );
                            }
                        }
                        StatsWriterMessage::QueryStatsCalendar {
                            request_id,
                            year,
                            completion,
                        } => {
                            // Issue #960：与 Explorer 同口径——先补写尚未落盘的编辑
                            // 事实，再让 Core 生成该年的逐日人工输入快照。
                            if let Some(err) = pending_records_failure(&api, &mut pending_records) {
                                completion(request_id, Err(err));
                            } else {
                                completion(
                                    request_id,
                                    api.get_writing_stats_heatmap_json(year)
                                        .map_err(|e| e.to_string()),
                                );
                            }
                        }
                    }
                }
                // 退出前最后再补写一次，尽量不丢尾部失败事实。
                let _ = flush_pending_records(&api, &mut pending_records);
            })
            .expect("failed to spawn stats-writer thread");
        Self {
            sender: Some(sender),
            worker: Some(worker),
        }
    }

    /// UI 线程调用：把命令放进 channel，立刻返回。
    ///
    /// Issue #935：发送失败（worker 已退出）不再静默丢弃，明确返回上层，
    /// 由调用方决定显示错误还是降级，避免统计事实无声丢失。
    pub(crate) fn enqueue(&self, cmd: StatsWriteCommand) -> Result<(), String> {
        let Some(sender) = self.sender.as_ref() else {
            return Err("stats writer is shut down".to_string());
        };
        sender
            .send(StatsWriterMessage::Record(cmd))
            .map_err(|_| "stats writer worker has exited".to_string())
    }

    /// Issue #935：把「统计面板快照」查询排进同一条串行队列。
    ///
    /// worker 消费到这条消息时，之前全部 Record 已处理完，它在 worker thread
    /// 里一次性生成快照并通过 completion 回报；UI 线程不再 `barrier().recv()`。
    pub(crate) fn query_dashboard(
        &self,
        request_id: u32,
        completion: StatsQueryCallback,
    ) -> Result<(), String> {
        let Some(sender) = self.sender.as_ref() else {
            return Err("stats writer is shut down".to_string());
        };
        sender
            .send(StatsWriterMessage::QueryDashboard {
                request_id,
                completion,
            })
            .map_err(|_| "stats writer worker has exited".to_string())
    }

    /// Issue #935：把「实时速度 + 今日汇总」查询排进同一条串行队列。
    pub(crate) fn query_writing_status(
        &self,
        request_id: u32,
        window_seconds: u32,
        completion: StatsQueryCallback,
    ) -> Result<(), String> {
        let Some(sender) = self.sender.as_ref() else {
            return Err("stats writer is shut down".to_string());
        };
        sender
            .send(StatsWriterMessage::QueryWritingStatus {
                request_id,
                window_seconds,
                completion,
            })
            .map_err(|_| "stats writer worker has exited".to_string())
    }

    /// Issue #946：把「统计页 Explorer 快照」查询排进同一条串行队列。
    ///
    /// 与 [`Self::query_dashboard`] 同一队列口径：worker 消费到它时此前全部
    /// Record 已处理完，在 worker thread 里调用 Core 的 Explorer 入口，
    /// UI 线程不阻塞。
    pub(crate) fn query_stats_explorer(
        &self,
        request_id: u32,
        preset: String,
        start: Option<String>,
        end: Option<String>,
        completion: StatsQueryCallback,
    ) -> Result<(), String> {
        let Some(sender) = self.sender.as_ref() else {
            return Err("stats writer is shut down".to_string());
        };
        sender
            .send(StatsWriterMessage::QueryStatsExplorer {
                request_id,
                preset,
                start,
                end,
                completion,
            })
            .map_err(|_| "stats writer worker has exited".to_string())
    }

    /// Issue #960：把「指定公历年的每日码字日历」查询排进同一条串行队列。
    ///
    /// 与 [`Self::query_stats_explorer`] 同一队列口径：worker 消费到它时此前全部
    /// Record 已处理完，在 worker thread 里调用 Core 的每日快照入口。
    pub(crate) fn query_stats_calendar(
        &self,
        request_id: u32,
        year: i32,
        completion: StatsQueryCallback,
    ) -> Result<(), String> {
        let Some(sender) = self.sender.as_ref() else {
            return Err("stats writer is shut down".to_string());
        };
        sender
            .send(StatsWriterMessage::QueryStatsCalendar {
                request_id,
                year,
                completion,
            })
            .map_err(|_| "stats writer worker has exited".to_string())
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
