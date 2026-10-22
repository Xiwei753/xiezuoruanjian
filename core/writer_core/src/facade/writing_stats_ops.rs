use crate::error::Result;
use crate::writing_stats::projection::CurrentWritingSpeed;
use crate::writing_stats::{DateRange, EventSource, WritingInputEvent};

use serde_json::Value;

impl super::WriterCore {
    /// 按编辑事务上报写作统计（平台端编辑事务与统计同源的正式入口）。
    ///
    /// 平台端只透传「编辑事实」：编辑 cause + contentDelta 的 inserted/deleted，
    /// 不自己猜 source。`cause → EventSource` 和「各计数字段怎么分配」这套业务规则
    /// 由 Core 单侧决定，避免每个平台各写一份、慢慢分叉。
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
    ///
    /// 不再接受 `duration_seconds` 和 `session_id` 参数：事件只提供字数事实，
    /// 真实时长由平台端通过 [`Self::record_writing_activity_segment`] 上报。
    /// 设备身份从 Core 的 `current_device.json` 取。
    ///
    /// `event_id` / `occurred_at_ms` 由平台端为编辑事务固定并原样重发：同一
    /// `event_id` 只落一条、只计一次；`occurred_at_ms` 是事件发生时间，
    /// 重发也不会把旧输入改记到今天（空串 / 0 表示老客户端，按 Core 旧行为生成）。
    #[allow(clippy::too_many_arguments)]
    pub fn record_editor_change_stats(
        &self,
        platform_str: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        cause: crate::editor::EditorTransactionCause,
        inserted_chars: u32,
        deleted_chars: u32,
        event_id: &str,
        occurred_at_ms: i64,
    ) -> Result<()> {
        use crate::editor::EditorTransactionCause;

        // 设备身份由 Core 读/建，不让平台端传；平台初始化注入的稳定 ID 优先。
        let platform = writer_platform_api::PlatformKind::from_str_name(platform_str)
            .unwrap_or(writer_platform_api::PlatformKind::Desktop);
        let preferred_device_id = self.preferred_device_id();
        let device_info = self.ensure_device_info(
            platform.to_str_name(),
            platform.default_device_class(),
            preferred_device_id.as_deref(),
        )?;
        let device_id = device_info.device_id.as_str();
        let device_class = device_info.device_class.as_str();

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

        let event = WritingInputEvent::with_identity(
            event_id.to_string(),
            occurred_at_ms,
            device_id,
            platform,
            device_class,
            project_id,
            volume_id,
            chapter_id,
            source,
            inserted,
            deleted,
            pasted,
            0,  // ai_inserted_chars
            0,  // duration_seconds — 已弃用；真实时长只来自 writing activity segment
            "", // session_id — 真实计时切片上报，不从事件时间差推算
        );

        self.get_stats_api().record_event(event)
    }

    /// 上报一段平台前台状态测得的写作计时切片。
    ///
    /// 平台端只传本机观察到的真实前台区间；设备身份、设备类别和本地日期
    /// 都由 Core 补齐。Core 按 `event_id` 幂等去重，重发不会重复累计。
    pub fn record_writing_activity_segment(
        &self,
        input: crate::api::WritingActivitySegmentInputDto,
    ) -> Result<()> {
        let platform = writer_platform_api::PlatformKind::from_str_name(&input.platform)
            .unwrap_or(writer_platform_api::PlatformKind::Desktop);
        let preferred_device_id = self.preferred_device_id();
        let device_info = self.ensure_device_info(
            platform.to_str_name(),
            platform.default_device_class(),
            preferred_device_id.as_deref(),
        )?;
        let segment = crate::writing_stats::WritingActivitySegment::new(
            input.event_id,
            &device_info.device_id,
            platform,
            &device_info.device_class,
            &input.project_id,
            &input.volume_id,
            &input.chapter_id,
            &input.session_id,
            input.start_at_ms,
            input.end_at_ms,
            &input.pause_reason,
        )?;
        self.get_stats_api().record_activity_segment(segment)
    }

    pub fn get_writing_stats_summary(&self, start_date: &str, end_date: &str) -> Result<Value> {
        let range = DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        };
        self.get_stats_api().get_stats_summary(&range)
    }

    /// 「今天」的写作汇总。今天是哪一天由 **Core 的本地日历口径**决定。
    pub fn get_today_writing_stats_summary(&self) -> Result<Value> {
        self.get_stats_api().get_today_stats_summary()
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
    pub fn get_current_writing_speed(&self, window_seconds: u32) -> Result<CurrentWritingSpeed> {
        self.get_stats_api().get_current_speed(window_seconds)
    }

    /// 单作品写作档案（#934）：作品元数据 + 历史累计人工码字 + 当前正文字数。
    ///
    /// 数据源语义分开：`created_at` 来自作品元数据；`current_word_count` 来自
    /// 现有章节存量（`get_project_stats`）；其余字段来自合并后的写作事件历史，
    /// `history_scope` 说明覆盖范围（仅本机 / 已同步设备 / 同步不完整）。
    /// 作品不存在时返回 `ProjectNotFound`，不返回「0 字作品」。
    pub fn get_project_writing_lifetime_stats(
        &self,
        project_id: &str,
    ) -> Result<super::ProjectWritingLifetimeStats> {
        let project = crate::project::get_project(self.projects_root(), project_id)?;
        let stats_api = self.get_stats_api();
        let lifetime = stats_api.get_project_lifetime_stats(project_id)?;
        let stats = self.get_project_stats(project_id)?;

        Ok(super::ProjectWritingLifetimeStats {
            project_id: project.id,
            created_at: project.created_at,
            first_written_at_ms: lifetime.first_written_at_ms,
            current_word_count: u64::from(stats.total_word_count),
            total_human_typed_chars: lifetime.total_human_typed_chars,
            best_writing_day: lifetime.best_writing_day,
            best_writing_day_chars: lifetime.best_writing_day_chars,
            writing_days: lifetime.writing_days,
            history_scope: stats_api.history_scope(),
        })
    }

    /// 统计面板一次性快照（#935）：读一次事件，按 Core 本地日历切分
    /// 今日 / 本周 / 本月，并给出本周范围的作品、章节、设备排行与速度曲线。
    pub fn get_writing_stats_dashboard(&self) -> Result<Value> {
        self.get_stats_api().get_dashboard_snapshot()
    }

    /// 统计页 Explorer 一次性快照（#944）：生涯摘要 + 四张时段卡 + 选中时段详情。
    ///
    /// 一次读盘覆盖全部时段；`preset` 取 `"last7Days"` / `"last30Days"` /
    /// `"last6Months"` / `"lifetime"` / `"custom"`，`custom` 必须带
    /// `custom_start` / `custom_end`（两端包含）。日期口径、纪录与趋势口径
    /// 全在 Core，端侧不传「今天」也不自己统计。
    pub fn get_writing_stats_explorer(
        &self,
        preset: &str,
        custom_start: Option<&str>,
        custom_end: Option<&str>,
    ) -> Result<Value> {
        let snapshot =
            self.get_stats_api()
                .get_explorer_snapshot(preset, custom_start, custom_end)?;
        serde_json::to_value(snapshot).map_err(Into::into)
    }

    /// 每日码字日历（#956）：指定公历年的逐日人工输入真值。
    ///
    /// 与 Explorer 同一份合并去重事件投影，端侧只做位置和色彩渲染；
    /// 非法年份返回错误，不静默补全年零数据。
    pub fn get_writing_stats_heatmap(
        &self,
        year: i32,
    ) -> Result<crate::api::types::WritingStatsHeatmapDto> {
        self.get_stats_api().get_heatmap_snapshot(year)
    }
}
