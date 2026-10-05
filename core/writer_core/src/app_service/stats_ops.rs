use crate::api::{
    ChapterStatsSummaryDto, CurrentWritingSpeedDto, DeviceInfoDto, DeviceStatsSummaryDto,
    EditorChangeStatsInputDto, ProjectStatsSummaryDto, SpeedCurveSummaryDto, WriterError,
    WritingStatsSummaryDto,
};

impl super::WriterAppService {
    /// 「今天」的写作汇总。日历日由 Core 本地时区口径决定，平台端不传日期。
    pub fn get_today_writing_stats_summary(&self) -> Result<WritingStatsSummaryDto, WriterError> {
        self.api.get_today_writing_stats_summary()
    }

    pub fn get_writing_stats_summary(
        &self,
        start_date: String,
        end_date: String,
    ) -> Result<WritingStatsSummaryDto, WriterError> {
        self.api.get_writing_stats_summary(&start_date, &end_date)
    }

    pub fn get_writing_stats_by_project(
        &self,
        start_date: String,
        end_date: String,
    ) -> Result<ProjectStatsSummaryDto, WriterError> {
        self.api
            .get_writing_stats_by_project(&start_date, &end_date)
    }

    pub fn get_writing_stats_by_chapter(
        &self,
        start_date: String,
        end_date: String,
    ) -> Result<ChapterStatsSummaryDto, WriterError> {
        self.api
            .get_writing_stats_by_chapter(&start_date, &end_date)
    }

    pub fn get_writing_stats_by_device(
        &self,
        start_date: String,
        end_date: String,
    ) -> Result<DeviceStatsSummaryDto, WriterError> {
        self.api.get_writing_stats_by_device(&start_date, &end_date)
    }

    pub fn get_writing_speed_curve(
        &self,
        start_date: String,
        end_date: String,
        bucket_minutes: u32,
    ) -> Result<SpeedCurveSummaryDto, WriterError> {
        self.api
            .get_writing_speed_curve(&start_date, &end_date, bucket_minutes)
    }

    /// 实时写作速度（最近 `window_seconds` 秒）。见
    /// [`WriterCore::get_current_writing_speed`](crate::facade::WriterCore::get_current_writing_speed)：
    /// 停笔超过一个窗口后自然回落到 0，不拿速度曲线最后一桶顶替。
    pub fn get_current_writing_speed(
        &self,
        window_seconds: u32,
    ) -> Result<CurrentWritingSpeedDto, WriterError> {
        self.api.get_current_writing_speed(window_seconds)
    }

    /// 按编辑事务上报写作统计。平台端只透传编辑事实（cause + contentDelta），
    /// source 分类由 Core 从 cause 推导。见
    /// [`WriterCore::record_editor_change_stats`](crate::facade::WriterCore::record_editor_change_stats)。
    pub fn record_editor_change_stats(
        &self,
        input: EditorChangeStatsInputDto,
    ) -> Result<(), WriterError> {
        self.api.record_editor_change_stats(input)
    }

    pub fn calculate_word_count(&self, text: String) -> u32 {
        self.api.calculate_word_count(&text)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_writing_event(
        &self,
        device_id: String,
        platform: String,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        old_text: String,
        new_text: String,
        duration_seconds: u32,
        session_id: String,
    ) -> Result<bool, WriterError> {
        self.api.process_writing_event(
            &device_id,
            &platform,
            &project_id,
            &volume_id,
            &chapter_id,
            &old_text,
            &new_text,
            duration_seconds,
            &session_id,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_writing_event(
        &self,
        device_id: String,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        source: String,
        inserted_chars: i32,
        deleted_chars: i32,
        pasted_chars: i32,
        ai_inserted_chars: i32,
        duration_seconds: i32,
        session_id: String,
    ) -> Result<bool, WriterError> {
        self.api.record_writing_event(
            &device_id,
            &project_id,
            &volume_id,
            &chapter_id,
            &source,
            inserted_chars,
            deleted_chars,
            pasted_chars,
            ai_inserted_chars,
            duration_seconds,
            &session_id,
        )
    }

    pub fn flush_writing_stats(&self) -> Result<bool, WriterError> {
        self.api.flush_writing_stats()
    }

    pub fn ensure_device_info(
        &self,
        platform: String,
        device_class: String,
    ) -> Result<bool, WriterError> {
        let preferred_id = self.device_id().map(|s| s.to_string());
        self.api
            .core_write()
            .ensure_device_info(&platform, &device_class, preferred_id.as_deref())
            .map(|_| true)
            .map_err(WriterError::from)
    }

    pub fn load_device_info(&self) -> Result<DeviceInfoDto, WriterError> {
        self.api.load_device_info()
    }
}
