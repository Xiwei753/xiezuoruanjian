use crate::api::{
    ChapterStatsSummaryDto, DeviceInfoDto, DeviceStatsSummaryDto, ProjectStatsSummaryDto,
    SpeedCurveSummaryDto, WriterError, WritingStatsSummaryDto,
};

impl super::WriterAppService {
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
