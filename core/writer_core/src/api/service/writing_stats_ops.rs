use super::*;

impl WriterCoreApi {
    /// 按编辑事务上报写作统计。平台端透传编辑事实，source 分类由 Core 决定。
    pub fn record_editor_change_stats(
        &self,
        input: crate::api::EditorChangeStatsInputDto,
    ) -> ApiResult<()> {
        self.core_write()
            .record_editor_change_stats(
                &input.platform,
                &input.project_id,
                &input.volume_id,
                &input.chapter_id,
                input.cause.into(),
                input.inserted_chars,
                input.deleted_chars,
            )
            .map_err(WriterError::from)
    }

    /// 「今天」的写作汇总。日历日由 Core 本地时区口径决定，平台端不传日期。
    pub fn get_today_writing_stats_summary(
        &self,
    ) -> ApiResult<crate::api::types::WritingStatsSummaryDto> {
        let value = self
            .core_write()
            .get_today_writing_stats_summary()
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
    }

    pub fn get_today_writing_stats_summary_json(&self) -> ApiResult<String> {
        let value = self.get_today_writing_stats_summary()?;
        Self::json_string(&value)
    }

    pub fn get_writing_stats_summary_json(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<String> {
        let value = self
            .core_write()
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
            .core_write()
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
            .core_write()
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
            .core_write()
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
            .core_write()
            .get_writing_speed_curve(start_date, end_date, bucket_minutes)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    /// 「当前写作速度」：以调用时刻为终点，回看 `window_seconds` 秒的纯输入速度。
    #[allow(clippy::cast_possible_truncation)]
    pub fn get_current_writing_speed(
        &self,
        window_seconds: u32,
    ) -> ApiResult<crate::api::types::CurrentWritingSpeedDto> {
        let speed = self
            .core_write()
            .get_current_writing_speed(window_seconds)
            .map_err(Into::<WriterError>::into)?;
        Ok(crate::api::types::CurrentWritingSpeedDto {
            window_seconds: speed.window_seconds,
            sampled_at_ms: speed.sampled_at_ms,
            chars_typed: speed.chars_typed,
            chars_per_minute: speed.chars_per_minute as f32,
        })
    }

    pub fn get_current_writing_speed_json(&self, window_seconds: u32) -> ApiResult<String> {
        let value = self.get_current_writing_speed(window_seconds)?;
        Self::json_string(&value)
    }

    pub fn calculate_word_count(&self, text: &str) -> u32 {
        self.core_write().calculate_word_count(text)
    }

    pub fn get_writing_stats_summary(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> ApiResult<crate::api::types::WritingStatsSummaryDto> {
        let value = self
            .core_write()
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
            .core_write()
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
            .core_write()
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
            .core_write()
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
            .core_write()
            .get_writing_speed_curve(start_date, end_date, bucket_minutes)
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
    }
}
