use super::*;

impl WriterCoreApi {
    /// 按编辑事务上报写作统计。平台端透传编辑事实，source 分类由 Core 决定。
    pub fn record_editor_change_stats(
        &self,
        input: crate::api::EditorChangeStatsInputDto,
    ) -> ApiResult<()> {
        self.core_write()
            .record_editor_change_stats(
                &input.device_id,
                &input.platform,
                &input.project_id,
                &input.volume_id,
                &input.chapter_id,
                input.cause.into(),
                input.inserted_chars,
                input.deleted_chars,
                input.duration_seconds,
                &input.session_id,
            )
            .map_err(WriterError::from)
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
    ///
    /// 停笔超过一个窗口后 `chars_per_minute` 自然回落到 0，所以平台端
    /// 直接展示这个值即可，不需要各自判断历史桶是否过期。
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

    #[allow(clippy::too_many_arguments)]
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
        self.core_write()
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
        let (platform, device_class) =
            if let Ok(info) = crate::settings::load_device_info(&self.app_data_root) {
                let p = if info.platform.is_empty() {
                    crate::writing_stats::Platform::Desktop
                } else {
                    crate::writing_stats::Platform::from_str_name(&info.platform)
                        .unwrap_or(crate::writing_stats::Platform::Desktop)
                };
                let dc = if info.device_class.is_empty() {
                    p.default_device_class().to_string()
                } else {
                    info.device_class
                };
                (p.to_str_name().to_string(), dc)
            } else {
                (
                    crate::writing_stats::Platform::Desktop
                        .to_str_name()
                        .to_string(),
                    crate::writing_stats::Platform::Desktop
                        .default_device_class()
                        .to_string(),
                )
            };

        self.core_write()
            .record_writing_event(
                device_id,
                &platform,
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
        let device_class = if let Ok(info) = crate::settings::load_device_info(&self.app_data_root)
        {
            if info.device_class.is_empty() {
                crate::writing_stats::Platform::from_str_name(platform)
                    .unwrap_or(crate::writing_stats::Platform::Desktop)
                    .default_device_class()
                    .to_string()
            } else {
                info.device_class
            }
        } else {
            crate::writing_stats::Platform::from_str_name(platform)
                .unwrap_or(crate::writing_stats::Platform::Desktop)
                .default_device_class()
                .to_string()
        };

        self.core_write()
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
        self.core_write()
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
