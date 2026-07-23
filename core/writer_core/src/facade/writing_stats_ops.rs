use crate::error::Result;
use crate::writing_stats::{DateRange, EventSource, WritingInputEvent};

use serde_json::Value;

impl super::WriterCore {
    #[allow(clippy::too_many_arguments, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]
    pub fn process_writing_event(
        &self,
        device_id: &str,
        platform_str: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        old_text: &str,
        new_text: &str,
        duration_seconds: u32,
        session_id: &str,
    ) -> Result<()> {
        let device_class = self
            .load_device_info()
            .map(|info| info.device_class)
            .unwrap_or_else(|_| {
                writer_platform_api::PlatformKind::from_str_name(platform_str)
                    .unwrap_or(writer_platform_api::PlatformKind::Desktop)
                    .default_device_class()
                    .to_string()
            });

        let old_len = old_text.chars().count() as i32;
        let new_len = new_text.chars().count() as i32;
        let diff = new_len - old_len;

        if diff == 0 {
            return Ok(());
        }

        let mut source_str = "human_typed";
        let mut inserted = diff as u32;
        let mut deleted = 0;
        let mut pasted = 0;

        if diff > 0 {
            if diff > 20 {
                source_str = "pasted";
                pasted = diff as u32;
                inserted = 0;
            }
        } else {
            source_str = "deleted";
            deleted = diff.unsigned_abs();
            inserted = 0;
        }

        self.record_writing_event(
            device_id,
            platform_str,
            &device_class,
            project_id,
            volume_id,
            chapter_id,
            source_str,
            inserted,
            deleted,
            pasted,
            0,
            duration_seconds,
            session_id,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_writing_event(
        &self,
        device_id: &str,
        platform_str: &str,
        device_class: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        source_str: &str,
        inserted_chars: u32,
        deleted_chars: u32,
        pasted_chars: u32,
        ai_inserted_chars: u32,
        duration_seconds: u32,
        session_id: &str,
    ) -> Result<()> {
        let platform = writer_platform_api::PlatformKind::from_str_name(platform_str)
            .unwrap_or(writer_platform_api::PlatformKind::Desktop);
        let source = match source_str {
            "pasted" => EventSource::Pasted,
            "deleted" => EventSource::Deleted,
            "ai_inserted" => EventSource::AiInserted,
            "sync_remote" => EventSource::SyncRemote,
            _ => EventSource::HumanTyped,
        };

        let event = WritingInputEvent::new(
            device_id,
            platform,
            device_class,
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
        );

        self.get_stats_api().record_event(event)
    }

    pub fn flush_writing_stats(&self) -> Result<()> {
        self.get_stats_api().flush()
    }

    pub fn get_writing_stats_summary(&self, start_date: &str, end_date: &str) -> Result<Value> {
        let range = DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        };
        self.get_stats_api().get_stats_summary(&range)
    }

    pub fn get_writing_stats_by_project(&self, start_date: &str, end_date: &str) -> Result<Value> {
        let range = DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        };
        self.get_stats_api().get_stats_by_project(&range)
    }

    pub fn get_writing_stats_by_chapter(&self, start_date: &str, end_date: &str) -> Result<Value> {
        let range = DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        };
        self.get_stats_api().get_stats_by_chapter(&range)
    }

    pub fn get_writing_stats_by_device(&self, start_date: &str, end_date: &str) -> Result<Value> {
        let range = DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        };
        self.get_stats_api().get_stats_by_device(&range)
    }

    pub fn get_writing_stats_by_device_class(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Value> {
        let range = DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        };
        self.get_stats_api().get_stats_by_device_class(&range)
    }

    pub fn get_writing_speed_curve(
        &self,
        start_date: &str,
        end_date: &str,
        bucket_minutes: u32,
    ) -> Result<Value> {
        let range = DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        };
        self.get_stats_api().get_speed_curve(&range, bucket_minutes)
    }
}
