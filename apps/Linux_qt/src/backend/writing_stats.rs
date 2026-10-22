// =============================================================================
// writing_stats.rs — 写作统计上报与查询（从 editor_backend.rs 拆分）
// =============================================================================

use super::*;

/// Issue #981：Linux Qt 前台写作会话运行时。
///
/// `WritingSessionClock` 只接收单调毫秒，这里用进程启动后的 `Instant` 提供；
/// 墙钟只在会话第一次输入时锚定一次。统计数据提交走异步串行 writer，不在输入
/// 热路径做磁盘 I/O。
pub(crate) struct LinuxWritingSession {
    clock: writer_core::writing_stats::WritingSessionClock,
    started_at: std::time::Instant,
    /// 正文页 + 应用/窗口当前是否在前台可写。后台迟到的 IME 提交不得重新开始计时。
    foreground: bool,
}

impl Default for LinuxWritingSession {
    fn default() -> Self {
        Self {
            clock: writer_core::writing_stats::WritingSessionClock::new(),
            started_at: std::time::Instant::now(),
            foreground: false,
        }
    }
}

impl LinuxWritingSession {
    fn now_mono_ms(&self) -> i64 {
        i64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(i64::MAX)
    }

    fn now_wall_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0)
    }
}

impl AppBackend {
    fn enqueue_writing_activity_segments(
        &self,
        segments: Vec<writer_core::writing_stats::WritingActivitySegment>,
    ) {
        let _ = self.try_enqueue_writing_activity_segments(segments);
    }

    /// 把计时切片排进 stats writer；失败时保留具体原因，供退出流程拒绝关窗。
    fn try_enqueue_writing_activity_segments(
        &self,
        segments: Vec<writer_core::writing_stats::WritingActivitySegment>,
    ) -> Result<(), String> {
        if segments.is_empty() {
            return Ok(());
        }
        let Some(ref writer) = self.stats_writer else {
            return Ok(());
        };
        for segment in segments {
            let input = writer_core::api::WritingActivitySegmentInputDto {
                event_id: segment.event_id,
                platform: "linux".to_string(),
                project_id: segment.project_id,
                volume_id: segment.volume_id,
                chapter_id: segment.chapter_id,
                session_id: segment.session_id,
                start_at_ms: segment.start_at_ms,
                end_at_ms: segment.end_at_ms,
                pause_reason: segment.pause_reason,
            };
            if let Err(e) = writer.enqueue(
                crate::backend::stats_writer::StatsWriteCommand::ActivitySegment(input),
            ) {
                self.debug_error("stats", "record_writing_activity_segment_enqueue_failed", &e);
                return Err(e);
            }
        }
        Ok(())
    }

    /// Issue #981：首次人工输入开始计时；暂停后再次输入恢复计时。
    ///
    /// 只有真人输入/删除会走到这里；粘贴、撤销、格式化等事务不算人工输入，
    /// 也不启动写作计时。
    pub(crate) fn on_human_writing_edit(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) {
        let segments = {
            let mut session = self
                .writing_session
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !session.foreground {
                return;
            }
            let mono_ms = session.now_mono_ms();
            let wall_ms = LinuxWritingSession::now_wall_ms();
            session.clock.on_first_human_edit(
                mono_ms,
                wall_ms,
                project_id,
                volume_id,
                chapter_id,
                "",
                writer_core::writing_stats::Platform::Desktop,
                "",
            )
        };
        self.enqueue_writing_activity_segments(segments);
    }

    /// Issue #981：30 秒心跳把已完成切片异步落盘，保持会话继续。
    pub(crate) fn writing_session_heartbeat(&self) {
        let segments = {
            let mut session = self
                .writing_session
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let mono_ms = session.now_mono_ms();
            session.clock.on_heartbeat(
                mono_ms,
                "",
                writer_core::writing_stats::Platform::Desktop,
                "",
            )
        };
        self.enqueue_writing_activity_segments(segments);
    }

    /// Issue #981：切换应用/最小化/锁屏/离开正文/手动暂停，立即结束当前切片。
    pub(crate) fn pause_writing_session(&self, reason: &str) {
        let segments = {
            let mut session = self
                .writing_session
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            session.foreground = false;
            let mono_ms = session.now_mono_ms();
            session.clock.on_pause(
                mono_ms,
                reason,
                "",
                writer_core::writing_stats::Platform::Desktop,
                "",
            )
        };
        self.enqueue_writing_activity_segments(segments);
    }

    /// Issue #981：回到前台只解除暂停标记，不补算离开期间；等下一次输入/显式继续。
    pub(crate) fn resume_writing_session(&self) {
        let mut session = self
            .writing_session
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        session.foreground = true;
        session.clock.on_resume();
    }

    /// Issue #981：离开正文或退出应用，结束会话并落下最后一段切片。
    pub(crate) fn close_writing_session(&self, _reason: &str) {
        let segments = {
            let mut session = self
                .writing_session
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            session.foreground = false;
            let mono_ms = session.now_mono_ms();
            session
                .clock
                .on_close(
                    mono_ms,
                    "",
                    writer_core::writing_stats::Platform::Desktop,
                    "",
                )
        };
        self.enqueue_writing_activity_segments(segments);
    }

    /// Issue #981：正常退出时先结算当前切片并等 stats writer 落盘，确认成功后才
    /// 正式结束会话。barrier 失败时保持 Writing/Paused 状态与 foreground，用户继续
    /// 留在正文里仍会从刚才的结算点继续计时，不会因一次退出失败丢后续写作时长。
    pub(crate) fn close_writing_session_and_wait(&self, reason: &str) -> Result<(), String> {
        let mono_ms = {
            let session = self
                .writing_session
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            session.now_mono_ms()
        };
        let segments = {
            let mut session = self
                .writing_session
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            session.clock.flush(
                mono_ms,
                reason,
                "",
                writer_core::writing_stats::Platform::Desktop,
                "",
            )
        };
        self.try_enqueue_writing_activity_segments(segments)?;
        self.stats_barrier_result()?;

        // 前面已经用同一个单调时刻结算过当前片段，这里只做正式复位，不会再产生
        // 一段未确认的尾时长；若 Core 仍返回尾段，必须再 barrier 确认后才能退出。
        let tail = {
            let mut session = self
                .writing_session
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            session.foreground = false;
            session.clock.on_close(
                mono_ms,
                "",
                writer_core::writing_stats::Platform::Desktop,
                "",
            )
        };
        if !tail.is_empty() {
            self.try_enqueue_writing_activity_segments(tail)?;
            self.stats_barrier_result()?;
        }
        Ok(())
    }

    /// 按编辑事务上报写作统计。
    ///
    /// 平台端只透传编辑事实（cause + inserted/deleted），不自己拼 source、device_id、session_id。
    /// `cause → EventSource` 映射和设备身份管理由 Core 内部完成。
    ///
    /// Issue #843: 不再在 UI 线程同步写盘。只把命令 enqueue 到独立 worker thread，
    /// 避免每次按键的文件系统 I/O 阻塞输入热路径。
    pub(crate) fn record_editor_change_stats(
        &self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        cause: writer_core::editor::EditorTransactionCause,
        inserted_chars: u32,
        deleted_chars: u32,
    ) {
        if let Some(ref writer) = self.stats_writer {
            // Issue #935 复核：编辑事实首次入队就固定稳定身份与发生时间，
            // 写盘失败重发时由 worker 原样带回，Core 按 event_id 判重。
            let event_id = uuid::Uuid::new_v4().to_string();
            let occurred_at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
                .unwrap_or(0);
            if let Err(e) = writer.enqueue(crate::backend::stats_writer::StatsWriteCommand::Editor {
                project_id: project_id.to_string(),
                volume_id: volume_id.to_string(),
                chapter_id: chapter_id.to_string(),
                cause,
                inserted_chars,
                deleted_chars,
                event_id,
                occurred_at_ms,
            }) {
                // Issue #935：发送失败不再静默丢弃，明确记诊断（统计事实已丢）。
                self.debug_error("stats", "record_editor_change_stats_enqueue_failed", &e);
            }
        }
    }

    /// Issue #843 复核评论 6045207375：查询统计前先等 writer 把队列中的 Record
    /// 全部落盘，避免读到旧数据或撞上正在追加的行。
    ///
    /// 写入在 worker thread，查询在 UI 线程用另一份 `WriterCoreApi`，两份 API 的
    /// 内部锁不是同一把锁，对同一份 `events.local/*.jsonl` 没有顺序保证。barrier
    /// 让 worker 处理到 Barrier 消息时前面所有 Record 已落盘，回 ack 后 UI 线程
    /// 再读磁盘。
    ///
    /// 返回 `true` 表示 barrier 成功（或没有 writer，直接通过）；
    /// 返回 `false` 表示 worker 已退出，调用方应走明确错误/空结果，不能拿旧磁盘
    /// 数据冒充最新结果。
    fn stats_barrier(&self) -> bool {
        self.stats_barrier_result().is_ok()
    }

    /// 与 [`Self::stats_barrier`] 相同，但保留 worker 回报的具体失败原因，
    /// 供退出流程提示用户“统计未保存”，而不是静默关窗。
    fn stats_barrier_result(&self) -> Result<(), String> {
        if let Some(ref writer) = self.stats_writer {
            writer.barrier()
        } else {
            Ok(())
        }
    }

    pub(crate) fn get_writing_stats_summary(
        &self,
        start_date: QString,
        end_date: QString,
    ) -> QString {
        // Issue #843 复核评论 6045207375：查询前先等 writer 把队列中的 Record 全部落盘。
        if !self.stats_barrier() {
            return "{\"error\":\"stats_writer_barrier_failed\"}".into();
        }
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_summary_json(&sd, &ed) {
                Ok(val) => val.into(),
                Err(e) => {
                    self.debug_error("stats", "get_writing_stats_summary_failed", &e.to_string());
                    format!("{{\"error\":\"{}\"}}", e.to_string().replace('"', "\\\"")).into()
                }
            }
        } else {
            "{\"error\":\"core not available\"}".into()
        }
    }

    /// 「今天」的写作汇总。
    ///
    /// 不传日期：Core 用自己的本地日历口径决定「今天是几号」。QML 侧
    /// `todayDateString()` 那种本地日期拼装一旦和 Core 的时区口径错开，
    /// 凌晨就会出现「今日进度提前清零」（Issue #829）。
    pub(crate) fn get_today_writing_stats_summary_object(&self) -> QJsonObject {
        // Issue #843 复核评论 6045207375：查询前先等 writer 把队列中的 Record 全部落盘。
        if !self.stats_barrier() {
            return QJsonObject::default();
        }
        if let Some(core) = self.core_api() {
            match core.get_today_writing_stats_summary_json() {
                Ok(val) => qjson_object_from_json(&val),
                Err(e) => {
                    self.debug_error(
                        "stats",
                        "get_today_writing_stats_summary_failed",
                        &e.to_string(),
                    );
                    QJsonObject::default()
                }
            }
        } else {
            QJsonObject::default()
        }
    }

    pub(crate) fn get_writing_stats_summary_object(
        &self,
        start_date: QString,
        end_date: QString,
    ) -> QJsonObject {
        // Issue #843 复核评论 6045207375：查询前先等 writer 把队列中的 Record 全部落盘。
        if !self.stats_barrier() {
            return QJsonObject::default();
        }
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_summary_json(&sd, &ed) {
                Ok(val) => qjson_object_from_json(&val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

    pub(crate) fn flush_recent_edits(&self) {
        if let Some(core) = self.core_api() {
            let _ = core.flush_recent_edits();
        }
    }

    pub(crate) fn get_writing_stats_by_project(
        &self,
        start_date: QString,
        end_date: QString,
    ) -> QString {
        // Issue #843 复核评论 6045207375：查询前先等 writer 把队列中的 Record 全部落盘。
        if !self.stats_barrier() {
            return "{\"error\":\"stats_writer_barrier_failed\"}".into();
        }
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_by_project_json(&sd, &ed) {
                Ok(val) => val.into(),
                Err(e) => {
                    self.debug_error(
                        "stats",
                        "get_writing_stats_by_project_failed",
                        &e.to_string(),
                    );
                    format!("{{\"error\":\"{}\"}}", e.to_string().replace('"', "\\\"")).into()
                }
            }
        } else {
            "{\"error\":\"core not available\"}".into()
        }
    }

    pub(crate) fn get_writing_stats_by_project_object(
        &self,
        start_date: QString,
        end_date: QString,
    ) -> QJsonObject {
        // Issue #843 复核评论 6045207375：查询前先等 writer 把队列中的 Record 全部落盘。
        if !self.stats_barrier() {
            return QJsonObject::default();
        }
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_by_project_json(&sd, &ed) {
                Ok(val) => qjson_object_from_json(&val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

    pub(crate) fn get_writing_stats_by_chapter(
        &self,
        start_date: QString,
        end_date: QString,
    ) -> QString {
        // Issue #843 复核评论 6045207375：查询前先等 writer 把队列中的 Record 全部落盘。
        if !self.stats_barrier() {
            return "{\"error\":\"stats_writer_barrier_failed\"}".into();
        }
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_by_chapter_json(&sd, &ed) {
                Ok(val) => val.into(),
                Err(e) => {
                    self.debug_error(
                        "stats",
                        "get_writing_stats_by_chapter_failed",
                        &e.to_string(),
                    );
                    format!("{{\"error\":\"{}\"}}", e.to_string().replace('"', "\\\"")).into()
                }
            }
        } else {
            "{\"error\":\"core not available\"}".into()
        }
    }

    pub(crate) fn get_writing_stats_by_chapter_object(
        &self,
        start_date: QString,
        end_date: QString,
    ) -> QJsonObject {
        // Issue #843 复核评论 6045207375：查询前先等 writer 把队列中的 Record 全部落盘。
        if !self.stats_barrier() {
            return QJsonObject::default();
        }
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_by_chapter_json(&sd, &ed) {
                Ok(val) => qjson_object_from_json(&val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

    pub(crate) fn get_writing_stats_by_device(
        &self,
        start_date: QString,
        end_date: QString,
    ) -> QString {
        // Issue #843 复核评论 6045207375：查询前先等 writer 把队列中的 Record 全部落盘。
        if !self.stats_barrier() {
            return "{\"error\":\"stats_writer_barrier_failed\"}".into();
        }
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_by_device_json(&sd, &ed) {
                Ok(val) => val.into(),
                Err(e) => {
                    self.debug_error(
                        "stats",
                        "get_writing_stats_by_device_failed",
                        &e.to_string(),
                    );
                    format!("{{\"error\":\"{}\"}}", e.to_string().replace('"', "\\\"")).into()
                }
            }
        } else {
            "{\"error\":\"core not available\"}".into()
        }
    }

    pub(crate) fn get_writing_stats_by_device_object(
        &self,
        start_date: QString,
        end_date: QString,
    ) -> QJsonObject {
        // Issue #843 复核评论 6045207375：查询前先等 writer 把队列中的 Record 全部落盘。
        if !self.stats_barrier() {
            return QJsonObject::default();
        }
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_by_device_json(&sd, &ed) {
                Ok(val) => qjson_object_from_json(&val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

    pub(crate) fn get_writing_speed_curve(
        &self,
        start_date: QString,
        end_date: QString,
        bucket_minutes: u32,
    ) -> QString {
        // Issue #843 复核评论 6045207375：查询前先等 writer 把队列中的 Record 全部落盘。
        if !self.stats_barrier() {
            return "{\"error\":\"stats_writer_barrier_failed\"}".into();
        }
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_speed_curve_json(&sd, &ed, bucket_minutes) {
                Ok(val) => val.into(),
                Err(e) => {
                    self.debug_error("stats", "get_writing_speed_curve_failed", &e.to_string());
                    format!("{{\"error\":\"{}\"}}", e.to_string().replace('"', "\\\"")).into()
                }
            }
        } else {
            "{\"error\":\"core not available\"}".into()
        }
    }

    pub(crate) fn get_writing_speed_curve_object(
        &self,
        start_date: QString,
        end_date: QString,
        bucket_minutes: u32,
    ) -> QJsonObject {
        // Issue #843 复核评论 6045207375：查询前先等 writer 把队列中的 Record 全部落盘。
        if !self.stats_barrier() {
            return QJsonObject::default();
        }
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_speed_curve_json(&sd, &ed, bucket_minutes) {
                Ok(val) => qjson_object_from_json(&val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

    /// 实时写作速度（最近 `window_seconds` 秒）。
    ///
    /// 和 `get_writing_speed_curve` 分工明确：速度曲线是历史分桶，桶只生成到
    /// 最后一个事件，拿它最后一桶当实时速度会在停笔后一直挂着非零值。Core 以
    /// 「现在」为终点重算窗口速度，停笔超过一个窗口后自然回落到 0，端侧不需要
    /// 自己判断历史桶是否过期，也不需要为刷新它强制 flush 统计事件。
    pub(crate) fn get_current_writing_speed(&self, window_seconds: u32) -> QJsonObject {
        // Issue #843 复核评论 6045207375：查询前先等 writer 把队列中的 Record 全部落盘。
        if !self.stats_barrier() {
            return QJsonObject::default();
        }
        if let Some(core) = self.core_api() {
            match core.get_current_writing_speed_json(window_seconds) {
                Ok(val) => qjson_object_from_json(&val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

    /// Issue #935：异步请求统计面板快照。
    ///
    /// 查询排进与写入相同的串行队列，worker 消费到它时此前全部编辑事实已处理完，
    /// 一次性生成快照后通过 completion 交回。本方法立即返回，UI 线程不再
    /// `barrier().recv()` 阻塞等写盘。completion 在 worker thread 上执行，调用方
    /// （editor_backend）负责用 `queued_callback` 把结果投递回 GUI 线程。
    pub(crate) fn request_writing_stats_dashboard(
        &self,
        request_id: u32,
        completion: crate::backend::stats_writer::StatsQueryCallback,
    ) -> Result<(), String> {
        let Some(writer) = self.stats_writer.as_ref() else {
            return Err("stats writer is not available".to_string());
        };
        writer.query_dashboard(request_id, completion)
    }

    /// Issue #935：异步请求「实时速度 + 今日汇总」。
    ///
    /// 与 [`Self::request_writing_stats_dashboard`] 同一队列口径；worker 返回
    /// `{"currentSpeed": {...}, "today": {...}}` 的组合快照。
    pub(crate) fn request_writing_status(
        &self,
        request_id: u32,
        window_seconds: u32,
        completion: crate::backend::stats_writer::StatsQueryCallback,
    ) -> Result<(), String> {
        let Some(writer) = self.stats_writer.as_ref() else {
            return Err("stats writer is not available".to_string());
        };
        writer.query_writing_status(request_id, window_seconds, completion)
    }

    /// Issue #946：异步请求统计页 Explorer 快照。
    ///
    /// 查询排进与写入相同的串行队列，worker 消费到它时此前全部编辑事实已处理完，
    /// 一次性生成「生涯摘要 + 四个时段卡 + 选中时段详情」后通过 completion 交回。
    /// 本方法立即返回；completion 在 worker thread 上执行，调用方
    /// （editor_backend）负责用 `queued_callback` 把结果投递回 GUI 线程。
    ///
    /// `custom` 必须携带 `start_date` / `end_date`（`YYYY-MM-DD`，两端包含）；
    /// 其余预设两端为 `None`。统计口径（预设范围、纪录、趋势）全在 Core。
    pub(crate) fn request_writing_stats_explorer(
        &self,
        request_id: u32,
        preset: String,
        start_date: Option<String>,
        end_date: Option<String>,
        completion: crate::backend::stats_writer::StatsQueryCallback,
    ) -> Result<(), String> {
        let Some(writer) = self.stats_writer.as_ref() else {
            return Err("stats writer is not available".to_string());
        };
        writer.query_stats_explorer(request_id, preset, start_date, end_date, completion)
    }

    /// Issue #960：异步请求指定公历年的每日码字日历快照。
    ///
    /// 与 [`Self::request_writing_stats_explorer`] 同一串行队列；Core 返回
    /// 该年 365/366 个真实业务日的 `humanTypedChars` / `intensity` / `status`，
    /// Qt 只用 Canvas 绘制，不在端侧重算每日字数。
    pub(crate) fn request_writing_stats_calendar(
        &self,
        request_id: u32,
        year: i32,
        completion: crate::backend::stats_writer::StatsQueryCallback,
    ) -> Result<(), String> {
        let Some(writer) = self.stats_writer.as_ref() else {
            return Err("stats writer is not available".to_string());
        };
        writer.query_stats_calendar(request_id, year, completion)
    }
}
