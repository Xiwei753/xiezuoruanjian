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
            .record_editor_transaction_stats(&input)
            .map_err(WriterError::from)
    }

    /// 按章节回放证明事件。`request_json` 为 `ProofReplayRequest` 的 camelCase
    /// JSON；返回目标时刻正文、选区、进度与链校验结果。播放器只渲染本结果，
    /// 不在平台端推演编辑规则。
    pub fn replay_chapter_proof_json(&self, request_json: &str) -> ApiResult<String> {
        let request =
            serde_json::from_str::<crate::writing_stats::replay::ProofReplayRequest>(request_json)
                .map_err(WriterError::from)?;
        let value = self
            .core_write()
            .replay_chapter_proof(&request)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    /// 导出章节证明包。包内包含历史正文，平台 UI 必须先明确提示用户。
    pub fn export_chapter_proof_json(
        &self,
        chapter_id: &str,
        device_id: &str,
    ) -> ApiResult<String> {
        let value = self
            .core_write()
            .export_chapter_proof(chapter_id, device_id)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    /// 列出某设备/章节的完整正文检查点。
    pub fn list_proof_checkpoints_json(
        &self,
        device_id: &str,
        chapter_id: &str,
    ) -> ApiResult<String> {
        let value = self
            .core_write()
            .list_proof_checkpoints(device_id, chapter_id)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    /// 上报一段平台已测量的前台写作计时切片。
    ///
    /// 平台端固定 `eventId` 并原样重发；Core 负责设备身份、本地日期切分和
    /// 幂等持久化，重发不会重复计时。
    pub fn record_writing_activity_segment(
        &self,
        input: crate::api::WritingActivitySegmentInputDto,
    ) -> ApiResult<()> {
        self.core_write()
            .record_writing_activity_segment(input)
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
        // 正文保存事务完成后再写检查点：checkpoint 是事件流的加速索引，
        // 失败不影响正文与已确认事件，但会明确返回错误供上层重试。
        self.core_write()
            .record_chapter_checkpoint(project_id, volume_id, chapter_id, content, "")?;
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
        let mut snapshot: crate::api::types::WritingStatsExplorerDto =
            serde_json::from_value(value).map_err(WriterError::from)?;
        let range = snapshot.selected.range.clone();
        snapshot.contributions = Some(self.get_contribution_stats(
            &range.start_date,
            &range.end_date,
            None,
            None,
            None,
            None,
        )?);
        Ok(snapshot)
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

    /// 贡献来源统计：历史操作量与当前保留内容来源一次查询返回。
    ///
    /// 只读已落盘的对象事件；MCP 写入的对象变更会在 Core 业务层记为 Ai。
    #[allow(clippy::too_many_arguments)]
    pub fn get_contribution_stats(
        &self,
        start_date: &str,
        end_date: &str,
        project_id: Option<&str>,
        actor: Option<&str>,
        object_type: Option<&str>,
        device_id: Option<&str>,
    ) -> ApiResult<crate::api::types::ContributionStatsDto> {
        if let Err(error) = crate::writing_stats::contribution_transaction::recover_pending(
            &self.app_data_root,
        ) {
            log::warn!("recover pending contribution transactions failed: {error}");
        }
        let current_texts = if object_type.is_none_or(|value| value == "starmap_node") {
            self.collect_current_starmap_texts(project_id)?
        } else {
            Vec::new()
        };
        let (aggregate, history_scope) = {
            let core = self.core_read();
            let stats = core.get_stats_api();
            (
                stats.get_contribution_stats(
                    start_date,
                    end_date,
                    project_id,
                    actor,
                    object_type,
                    device_id,
                    &current_texts,
                )?,
                stats.history_scope(),
            )
        };
        Ok(contribution_stats_dto(
            &aggregate,
            start_date,
            end_date,
            project_id,
            history_scope,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn get_contribution_stats_json(
        &self,
        start_date: &str,
        end_date: &str,
        project_id: Option<&str>,
        actor: Option<&str>,
        object_type: Option<&str>,
        device_id: Option<&str>,
    ) -> ApiResult<String> {
        let stats = self.get_contribution_stats(
            start_date,
            end_date,
            project_id,
            actor,
            object_type,
            device_id,
        )?;
        Self::json_string(&stats)
    }
}

fn contribution_counts_dto(
    counts: &crate::writing_stats::projection::ContributionCounts,
) -> crate::api::types::ContributionCountsDto {
    crate::api::types::ContributionCountsDto {
        inserted_chars: counts.inserted_chars,
        deleted_chars: counts.deleted_chars,
        modified_chars: counts.modified_chars,
        operations: counts.operations,
        created_objects: counts.created_objects,
        updated_objects: counts.updated_objects,
        deleted_objects: counts.deleted_objects,
        linked_objects: counts.linked_objects,
        unlinked_objects: counts.unlinked_objects,
    }
}

fn contribution_stats_dto(
    aggregate: &crate::writing_stats::projection::ContributionStatsAgg,
    start_date: &str,
    end_date: &str,
    project_id: Option<&str>,
    history_scope: String,
) -> crate::api::types::ContributionStatsDto {
    crate::api::types::ContributionStatsDto {
        range: crate::api::types::DateRangeDto {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        },
        project_id: project_id.map(str::to_string),
        total: contribution_counts_dto(&aggregate.total),
        actors: aggregate
            .actors
            .values()
            .map(|actor| crate::api::types::ContributionActorStatsDto {
                actor: actor.actor.clone(),
                counts: contribution_counts_dto(&actor.counts),
                retained_chars: actor.retained_chars,
            })
            .collect(),
        object_types: aggregate
            .object_types
            .values()
            .map(|object| crate::api::types::ContributionObjectTypeStatsDto {
                object_type: object.object_type.clone(),
                counts: contribution_counts_dto(&object.counts),
                retained_chars: object.retained_chars,
            })
            .collect(),
        days: aggregate
            .days
            .values()
            .map(|day| crate::api::types::ContributionDayStatsDto {
                date: day.date.clone(),
                counts: contribution_counts_dto(&day.counts),
            })
            .collect(),
        devices: aggregate
            .devices
            .values()
            .map(|device| crate::api::types::ContributionDeviceStatsDto {
                device_id: device.device_id.clone(),
                counts: contribution_counts_dto(&device.counts),
            })
            .collect(),
        retained_total_chars: aggregate.retained_total_chars,
        history_scope,
    }
}
