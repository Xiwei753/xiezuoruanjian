use super::*;

impl WriterCoreApi {
    pub fn get_writing_stats_summary_json(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_stats_summary(start_date, end_date)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_writing_stats_by_project_json(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_stats_by_project(start_date, end_date)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_writing_stats_by_chapter_json(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_stats_by_chapter(start_date, end_date)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_writing_stats_by_device_json(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_stats_by_device(start_date, end_date)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_writing_speed_curve_json(
        &self,
        start_date: &str,
        end_date: &str,
        bucket_minutes: u32,
    ) -> ApiResult<String> {
        let value = self
            .core()
            .get_writing_speed_curve(start_date, end_date, bucket_minutes)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn calculate_word_count(&self, text: &str) -> u32 {
        self.core().calculate_word_count(text)
    }

    pub fn process_writing_event(
        &self,
        device_id: &str,
        platform: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        old_text: &str,
        new_text: &str,
        duration_seconds: u32,
        session_id: &str,
    ) -> ApiResult<bool> {
        self.core()
            .process_writing_event(
                device_id,
                platform,
                project_id,
                volume_id,
                chapter_id,
                old_text,
                new_text,
                duration_seconds,
                session_id,
            )
            .map(|_| true)
            .map_err(WriterError::from)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_writing_event(
        &self,
        device_id: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        source: &str,
        inserted_chars: i32,
        deleted_chars: i32,
        pasted_chars: i32,
        ai_inserted_chars: i32,
        duration_seconds: i32,
        session_id: &str,
    ) -> ApiResult<bool> {
        // 校验非负计数器
        let inserted_chars = Self::non_negative_counter("inserted_chars", inserted_chars)?;
        let deleted_chars = Self::non_negative_counter("deleted_chars", deleted_chars)?;
        let pasted_chars = Self::non_negative_counter("pasted_chars", pasted_chars)?;
        let ai_inserted_chars = Self::non_negative_counter("ai_inserted_chars", ai_inserted_chars)?;
        let duration_seconds = Self::non_negative_counter("duration_seconds", duration_seconds)?;

        // 读取 current_device.json 获取 platform 和 device_class
        let (platform, device_class) = if let Ok(info) = crate::settings::load_device_info(
            &self.workspace_path,
        ) {
            let p = if info.platform.is_empty() { "android" } else { &info.platform };
            let dc = if info.device_class.is_empty() { "phone" } else { &info.device_class };
            (p.to_string(), dc.to_string())
        } else {
            ("android".to_string(), "phone".to_string())
        };

        self.core()
            .record_writing_event(
                device_id, &platform, &device_class,
                project_id, volume_id, chapter_id, source,
                inserted_chars, deleted_chars, pasted_chars,
                ai_inserted_chars, duration_seconds, session_id,
            )
            .map(|_| true)
            .map_err(WriterError::from)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_writing_event_for_platform(
        &self,
        device_id: &str,
        platform: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        source: &str,
        inserted_chars: i32,
        deleted_chars: i32,
        pasted_chars: i32,
        ai_inserted_chars: i32,
        duration_seconds: i32,
        session_id: &str,
    ) -> ApiResult<bool> {
        let inserted_chars = Self::non_negative_counter("inserted_chars", inserted_chars)?;
        let deleted_chars = Self::non_negative_counter("deleted_chars", deleted_chars)?;
        let pasted_chars = Self::non_negative_counter("pasted_chars", pasted_chars)?;
        let ai_inserted_chars = Self::non_negative_counter("ai_inserted_chars", ai_inserted_chars)?;
        let duration_seconds = Self::non_negative_counter("duration_seconds", duration_seconds)?;

        // 优先从 current_device.json 读取 device_class
        let device_class = if let Ok(info) = crate::settings::load_device_info(
            &self.workspace_path,
        ) {
            if info.device_class.is_empty() {
                // fallback：根据 platform 推断
                if platform == "android" {
                    "phone".to_string()
                } else if platform == "harmony" {
                    "tablet".to_string()
                } else {
                    "desktop".to_string()
                }
            } else {
                info.device_class
            }
        } else {
            // fallback：根据 platform 推断
            if platform == "android" {
                "phone".to_string()
            } else if platform == "harmony" {
                "tablet".to_string()
            } else {
                "desktop".to_string()
            }
        };

        self.core()
            .record_writing_event(
                device_id,
                platform,
                &device_class,
                project_id,
                volume_id,
                chapter_id,
                source,
                inserted_chars,
                deleted_chars,
                pasted_chars,
                ai_inserted_chars,
                duration_seconds,
                session_id,
            )
            .map(|_| true)
            .map_err(WriterError::from)
    }

    pub fn flush_writing_stats(&self) -> ApiResult<bool> {
        self.core()
            .flush_writing_stats()
            .map(|_| true)
            .map_err(WriterError::from)
    }

    pub fn get_writing_stats_summary(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<crate::api::types::WritingStatsSummaryDto> {
        let value = self
            .core()
            .get_writing_stats_summary(start_date, end_date)
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
    }

    pub fn get_writing_stats_by_project(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<crate::api::types::ProjectStatsSummaryDto> {
        let value = self
            .core()
            .get_writing_stats_by_project(start_date, end_date)
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
    }

    pub fn get_writing_stats_by_chapter(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<crate::api::types::ChapterStatsSummaryDto> {
        let value = self
            .core()
            .get_writing_stats_by_chapter(start_date, end_date)
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
    }

    pub fn get_writing_stats_by_device(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<crate::api::types::DeviceStatsSummaryDto> {
        let value = self
            .core()
            .get_writing_stats_by_device(start_date, end_date)
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
    }

    pub fn get_writing_speed_curve(
        &self,
        start_date: &str,
        end_date: &str,
        bucket_minutes: u32,
    ) -> ApiResult<crate::api::types::SpeedCurveSummaryDto> {
        let value = self
            .core()
            .get_writing_speed_curve(start_date, end_date, bucket_minutes)
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
    }
}
