// =============================================================================
// writing_stats.rs — 写作统计上报与查询（从 editor_backend.rs 拆分）
// =============================================================================

use super::*;

impl AppBackend {
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
            writer.enqueue(crate::backend::stats_writer::StatsWriteCommand {
                project_id: project_id.to_string(),
                volume_id: volume_id.to_string(),
                chapter_id: chapter_id.to_string(),
                cause,
                inserted_chars,
                deleted_chars,
            });
        }
    }

    pub(crate) fn get_writing_stats_summary(
        &self,
        start_date: QString,
        end_date: QString,
    ) -> QString {
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
        if let Some(core) = self.core_api() {
            match core.get_current_writing_speed_json(window_seconds) {
                Ok(val) => qjson_object_from_json(&val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }
}
