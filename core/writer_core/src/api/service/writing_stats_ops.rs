use super::*;

impl WriterCoreApi {
    /// 按编辑事务上报写作统计。平台端透传编辑事实，source 分类由 Core 决定。
    ///
    /// `eventId` / `occurredAtMs` 由平台端为事务固定并原样重发；
    /// Core 按 `event_id` 幂等去重，重发不会重复计字。
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
                &input.event_id,
                input.occurred_at_ms,
            )
            .map_err(WriterError::from)
    }

    /// 正文保存 + 未确认统计事件一次提交（按正文保存事务对齐）。
    ///
    /// 顺序：**先保存正文**（沿用既有的空覆盖保护、搜索索引与变更集逻辑），
    /// 成功后再逐条幂等记录事件。正文保存失败直接返回错误、事件一条不记；
    /// 事件记录只可能因磁盘错误失败，此时调用方保留原事件重发（同
    /// `event_id` 再次提交只补落缺失的事件，不会重复计字）。
    pub fn save_chapter_with_editor_stats(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
        events: Vec<crate::api::EditorChangeStatsInputDto>,
    ) -> ApiResult<crate::api::types::ChapterSaveReceiptDto> {
        let receipt = self
            .save_chapter_content_with_options(project_id, volume_id, chapter_id, content, false)?;
        for event in events {
            self.record_editor_change_stats(event)?;
        }
        Ok(receipt)
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

    /// 单作品写作档案（#934）：开写日期、累计人工码字、单日最高纪录。
    pub fn get_project_writing_lifetime_stats(
        &self,
        project_id: &str,
    ) -> ApiResult<crate::api::types::ProjectWritingLifetimeStatsDto> {
        let stats = self
            .core_write()
            .get_project_writing_lifetime_stats(project_id)
            .map_err(Into::<WriterError>::into)?;
        Ok(crate::api::types::ProjectWritingLifetimeStatsDto {
            project_id: stats.project_id,
            created_at: stats.created_at,
            first_written_at: stats
                .first_written_at_ms
                .and_then(chrono::DateTime::from_timestamp_millis)
                .map(|dt| dt.to_rfc3339()),
            current_word_count: stats.current_word_count,
            total_human_typed_chars: stats.total_human_typed_chars,
            best_writing_day: stats.best_writing_day,
            best_writing_day_chars: stats.best_writing_day_chars,
            writing_days: stats.writing_days,
            history_scope: stats.history_scope,
        })
    }

    pub fn get_project_writing_lifetime_stats_json(&self, project_id: &str) -> ApiResult<String> {
        let value = self.get_project_writing_lifetime_stats(project_id)?;
        Self::json_string(&value)
    }

    /// 统计面板一次性快照（#935）：一次读取，今日 / 本周 / 本月 + 本周排行 + 曲线。
    pub fn get_writing_stats_dashboard(
        &self,
    ) -> ApiResult<crate::api::types::WritingStatsDashboardDto> {
        let value = self
            .core_write()
            .get_writing_stats_dashboard()
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
    }

    pub fn get_writing_stats_dashboard_json(&self) -> ApiResult<String> {
        let value = self.get_writing_stats_dashboard()?;
        Self::json_string(&value)
    }

    /// 统计页 Explorer 一次性快照（#944）：生涯摘要 + 四张时段卡 + 选中时段详情。
    ///
    /// `preset` 取 `"last7Days"` / `"last30Days"` / `"last6Months"` /
    /// `"lifetime"` / `"custom"`；`custom_start` / `custom_end` 是可选的自选
    /// 闭区间（同给同给，`custom` 必给）。参数非法返回错误，不做静默兜底。
    pub fn get_writing_stats_explorer(
        &self,
        preset: &str,
        custom_start: Option<&str>,
        custom_end: Option<&str>,
    ) -> ApiResult<crate::api::types::WritingStatsExplorerDto> {
        let value = self
            .core_read()
            .get_writing_stats_explorer(preset, custom_start, custom_end)
            .map_err(Into::<WriterError>::into)?;
        serde_json::from_value(value).map_err(Into::into)
    }

    pub fn get_writing_stats_explorer_json(
        &self,
        preset: &str,
        custom_start: Option<&str>,
        custom_end: Option<&str>,
    ) -> ApiResult<String> {
        let snapshot = self.get_writing_stats_explorer(preset, custom_start, custom_end)?;
        Self::json_string(&snapshot)
    }

    /// 每日码字日历（#956）：指定公历年的逐日人工输入真值。
    ///
    /// 与 Explorer 同源：本机分区与已同步设备分区按 `event_id` 去重后的
    /// 全部已到达事件。非法年份返回结构化错误，不造零数据。
    pub fn get_writing_stats_heatmap(
        &self,
        year: i32,
    ) -> ApiResult<crate::api::types::WritingStatsHeatmapDto> {
        self.core_read()
            .get_writing_stats_heatmap(year)
            .map_err(Into::<WriterError>::into)
    }

    pub fn get_writing_stats_heatmap_json(&self, year: i32) -> ApiResult<String> {
        let snapshot = self.get_writing_stats_heatmap(year)?;
        Self::json_string(&snapshot)
    }
}
