use crate::error::Result;
use crate::writing_stats::store::CurrentWritingSpeed;
use crate::writing_stats::{DateRange, EventSource, WritingInputEvent};

use serde_json::Value;

/// 平台端传来的 `source` 字符串 → [`EventSource`]。
fn event_source_from_str(source_str: &str) -> EventSource {
    match source_str {
        "typing" => EventSource::HumanTyped,
        "pasted" => EventSource::Pasted,
        "deleted" => EventSource::Deleted,
        "ai_inserted" => EventSource::AiInserted,
        "sync_remote" => EventSource::SyncRemote,
        // Android 按 Core cause 明确分类后发送的字符串。
        // Undo/Redo/Programmatic/纯光标移动不是人工输入 — 显式映射为 Unknown
        // （不计入分类计数器，但仍计入 net_delta_chars），不得落入默认 HumanTyped。
        "undo" | "redo" | "programmatic" | "selection" => EventSource::Unknown,
        _ => EventSource::HumanTyped,
    }
}

impl super::WriterCore {
    #[allow(
        clippy::too_many_arguments,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap
    )]
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
        let source = event_source_from_str(source_str);
        self.record_writing_event_with_source(
            device_id,
            platform_str,
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
        )
    }

    /// [`Self::record_writing_event`] 的强类型版本：直接接受已分类好的 [`EventSource`]。
    ///
    /// 平台端如果已经拿到编辑事务的 cause，应该走
    /// [`Self::record_editor_change_stats`]（由 Core 从 cause 推导 source），
    /// 而不是自己拼 `source_str` 字符串。
    #[allow(clippy::too_many_arguments)]
    pub fn record_writing_event_with_source(
        &self,
        device_id: &str,
        platform_str: &str,
        device_class: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        source: EventSource,
        inserted_chars: u32,
        deleted_chars: u32,
        pasted_chars: u32,
        ai_inserted_chars: u32,
        duration_seconds: u32,
        session_id: &str,
    ) -> Result<()> {
        let platform = writer_platform_api::PlatformKind::from_str_name(platform_str)
            .unwrap_or(writer_platform_api::PlatformKind::Desktop);

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

    /// 按编辑事务上报写作统计（平台端编辑事务与统计同源的正式入口）。
    ///
    /// 平台端只透传「编辑事实」：编辑 cause + contentDelta 的 inserted/deleted，
    /// 不自己猜 source。`cause → EventSource` 和「各计数字段怎么分配」这套业务规则
    /// 由 Core 单侧决定，避免每个平台各写一份、慢慢分叉。
    ///
    /// ## 为什么不能只按整章 old/new 文本比较
    ///
    /// [`Self::process_writing_event`] 拿两份整章文本做 diff，套「净增 > 20 就当
    /// paste」的启发式。对着旧文本做 diff 既漏掉自上次统计以来的增量，又会把连续
    /// 敲的字误判成粘贴，结果是纯输入统计偏低、实时速度长期显示 0。
    ///
    /// ## 映射规则
    ///
    /// - `Typing` / `TypingCommit` / `ImeComposition` → `HumanTyped`，记 `inserted` / `deleted`。
    /// - `Paste` → `Pasted`，字数全部记 `pasted`，`inserted` 为 0。
    /// - `Delete` → `Deleted`，只记 `deleted`。
    /// - `Undo` / `Redo` / `Load` / `Format` / `Programmatic` → `Unknown`，
    ///   不是人工输入；仍按实际 delta 记 `inserted` / `deleted`，只是不计入分类计数器。
    ///
    /// 一次事件只让一个来源的「插入」字段非零，这样
    /// `net_delta_chars = inserted + pasted + ai_inserted - deleted` 才成立。
    pub fn record_editor_change_stats(
        &self,
        device_id: &str,
        platform_str: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        cause: crate::editor::EditorTransactionCause,
        inserted_chars: u32,
        deleted_chars: u32,
        duration_seconds: u32,
        session_id: &str,
    ) -> Result<()> {
        use crate::editor::EditorTransactionCause;

        let device_class = self
            .load_device_info()
            .map(|info| info.device_class)
            .unwrap_or_else(|_| {
                writer_platform_api::PlatformKind::from_str_name(platform_str)
                    .unwrap_or(writer_platform_api::PlatformKind::Desktop)
                    .default_device_class()
                    .to_string()
            });

        let (source, inserted, deleted, pasted) = match cause {
            EditorTransactionCause::Typing
            | EditorTransactionCause::TypingCommit
            | EditorTransactionCause::ImeComposition => {
                (EventSource::HumanTyped, inserted_chars, deleted_chars, 0)
            }
            EditorTransactionCause::Paste => {
                (EventSource::Pasted, 0, deleted_chars, inserted_chars)
            }
            EditorTransactionCause::Delete => (EventSource::Deleted, 0, deleted_chars, 0),
            EditorTransactionCause::Undo
            | EditorTransactionCause::Redo
            | EditorTransactionCause::Load
            | EditorTransactionCause::Format
            | EditorTransactionCause::Programmatic => {
                (EventSource::Unknown, inserted_chars, deleted_chars, 0)
            }
        };

        // 空事件不落盘：纯光标移动 / 无内容变化的事务不该污染会话时长。
        if inserted == 0 && deleted == 0 && pasted == 0 {
            return Ok(());
        }

        self.record_writing_event_with_source(
            device_id,
            platform_str,
            &device_class,
            project_id,
            volume_id,
            chapter_id,
            source,
            inserted,
            deleted,
            pasted,
            0,
            duration_seconds,
            session_id,
        )
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

    /// 「当前写作速度」：以调用时刻为终点的实时纯输入速度。
    ///
    /// 与 [`Self::get_writing_speed_curve`] 分工明确：速度曲线是历史分桶，
    /// 这里是以「现在」为终点重算的窗口速度，停笔超过一个窗口后回落到 0。
    /// 平台端展示实时速度一律走这里，不要拿曲线最后一桶顶替。
    pub fn get_current_writing_speed(&self, window_seconds: u32) -> Result<CurrentWritingSpeed> {
        self.get_stats_api()
            .aggregator()
            .get_current_speed(window_seconds)
    }
}
