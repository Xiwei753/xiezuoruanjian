// =============================================================================
// writing_stats.rs — 写作事件上报与统计查询（从 editor_backend.rs 拆分）
// =============================================================================

use super::*;

impl AppBackend {
    pub(crate) fn report_writing_event(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        source: QString,
        inserted_chars: u32,
        deleted_chars: u32,
        pasted_chars: u32,
    ) {
        let pid = project_id.to_string();
        let vid = volume_id.to_string();
        let cid = chapter_id.to_string();
        let src = source.to_string();

        if let Some(core) = self.core_api() {
            writing_bridge::ensure_stats_session(
                &core,
                &mut self.stats_device_id,
                &mut self.stats_session_id,
                &mut self.stats_last_event_ms,
            );

            if let Err(e) = writing_bridge::report_writing_event(
                &core,
                &pid,
                &vid,
                &cid,
                &src,
                inserted_chars,
                deleted_chars,
                pasted_chars,
                0,
                &self.stats_device_id,
                &self.stats_session_id,
            ) {
                self.debug_error("stats", "report_writing_event_failed", &e.to_string());
            }
            // NOTE: flush_writing_stats removed from per-keystroke path.
            // Stats are flushed on chapter save (saveCurrentChapter) and on app exit.
            // Per-keystroke disk I/O was the single biggest FFI performance bottleneck.
        }
    }

    pub(crate) fn process_writing_event_from_text(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        old_text: QString,
        new_text: QString,
    ) {
        let pid = project_id.to_string();
        let vid = volume_id.to_string();
        let cid = chapter_id.to_string();
        let ot = old_text.to_string();
        let nt = new_text.to_string();

        if let Some(core) = self.core_api() {
            writing_bridge::ensure_stats_session(
                &core,
                &mut self.stats_device_id,
                &mut self.stats_session_id,
                &mut self.stats_last_event_ms,
            );

            if let Err(e) = writing_bridge::process_writing_event_from_text(
                &core,
                &pid,
                &vid,
                &cid,
                &ot,
                &nt,
                &self.stats_device_id,
                &self.stats_session_id,
            ) {
                self.debug_error(
                    "stats",
                    "process_writing_event_from_text_failed",
                    &e.to_string(),
                );
            }
            // NOTE: flush_writing_stats removed from per-keystroke path.
            // Stats are flushed on chapter save and on app exit.
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

    pub(crate) fn flush_writing_stats(&self) {
        if let Some(core) = self.core_api() {
            let _ = core.flush_writing_stats();
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
}
