use crate::api::{
    ChapterStatsSummaryDto, CurrentWritingSpeedDto, DeviceInfoDto, DeviceStatsSummaryDto,
    EditorChangeStatsInputDto, ProjectStatsSummaryDto, ProjectWritingLifetimeStatsDto,
    SpeedCurveSummaryDto, WriterError, WritingStatsSummaryDto,
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

    /// 实时写作速度（最近 `window_seconds` 秒）。
    pub fn get_current_writing_speed(
        &self,
        window_seconds: u32,
    ) -> Result<CurrentWritingSpeedDto, WriterError> {
        self.api.get_current_writing_speed(window_seconds)
    }

    /// 按编辑事务上报写作统计。平台端只透传编辑事实（cause + contentDelta），
    /// source 分类由 Core 从 cause 推导。
    ///
    /// `event_id` / `occurred_at_ms` 由平台端为编辑事务固定并原样重发；
    /// Core 幂等去重，重发不重复计字、也不把旧事件改记到今天。
    pub fn record_editor_change_stats(
        &self,
        input: EditorChangeStatsInputDto,
    ) -> Result<(), WriterError> {
        self.api.record_editor_change_stats(input)
    }

    /// 正文保存 + 未确认统计事件一次提交（正文保存事务的统计对齐入口）。
    pub fn save_chapter_with_editor_stats(
        &self,
        project_id: String,
        volume_id: String,
        chapter_id: String,
        content: String,
        events: Vec<EditorChangeStatsInputDto>,
    ) -> Result<crate::api::types::ChapterSaveReceiptDto, WriterError> {
        self.api
            .save_chapter_with_editor_stats(&project_id, &volume_id, &chapter_id, &content, events)
    }

    pub fn calculate_word_count(&self, text: String) -> u32 {
        self.api.calculate_word_count(&text)
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

    /// 单作品写作档案（#934）：开写日期、累计人工码字、单日最高纪录。
    ///
    /// `history_scope` 说明历史只覆盖本地可读事件（`events.local` 不同步）。
    pub fn get_project_writing_lifetime_stats(
        &self,
        project_id: String,
    ) -> Result<ProjectWritingLifetimeStatsDto, WriterError> {
        self.api.get_project_writing_lifetime_stats(&project_id)
    }
}
