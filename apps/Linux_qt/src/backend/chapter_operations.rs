// =============================================================================
// chapter_operations.rs — 章节读写操作（从 editor_backend.rs 拆分）
// =============================================================================

use super::*;

impl AppBackend {
    pub(crate) fn get_chapter_content(
        &self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> QString {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        self.debug_log(
            "chapter",
            "get_chapter_content_start",
            &format!("project_id={}, volume_id={}, chapter_id={}", p, v, c),
        );
        if let Some(api) = self.core_api() {
            match api.open_chapter(&p, &v, &c) {
                Ok(content) => {
                    self.debug_log(
                        "chapter",
                        "get_chapter_content_success",
                        &format!("len={}", content.content.len()),
                    );
                    return content.content.into();
                }
                Err(e) => {
                    self.debug_error(
                        "chapter",
                        "get_chapter_content_failed",
                        &format!("error={}", e),
                    );
                }
            }
        } else {
            self.debug_error(
                "chapter",
                "get_chapter_content_failed",
                "core_not_initialized",
            );
        }
        "".into()
    }

    pub(crate) fn open_chapter_json(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> QString {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        if let Some(core) = self.core_api() {
            return match core.open_chapter(&p, &v, &c) {
                Ok(content) => {
                    let data = serde_json::json!({
                        "content": content.content,
                        "title": content.meta.title,
                        "projectId": p,
                        "volumeId": v,
                        "chapterId": c,
                        "meta": content.meta,
                    });
                    writer_core::api::ResultEnvelope::success(data)
                        .to_json_string()
                        .into()
                }
                Err(e) => writer_core::api::ResultEnvelope::<()>::error(
                    writer_core::api::WriterError::Io(e.to_string()),
                )
                .to_json_string()
                .into(),
            };
        }
        writer_core::api::ResultEnvelope::<()>::error(writer_core::api::WriterError::Other(
            "core api not available".to_string(),
        ))
        .to_json_string()
        .into()
    }

    pub(crate) fn open_chapter(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        self.debug_log(
            "chapter",
            "open_chapter_start",
            &format!("project_id={}, volume_id={}, chapter_id={}", p, v, c),
        );

        if let Some(core) = self.core_api() {
            match writing_bridge::open_chapter(&core, &p, &v, &c) {
                Ok(data) => {
                    // Issue #833：记录实际 content 字节长度/字符长度，
                    // 不再只写 "len_loaded" 占位字符串。以后再出现"正文空白"，
                    // 日志能立刻区分"Core 读到了正文"和"读到空串"。
                    self.debug_log(
                        "chapter",
                        "open_chapter_success",
                        &format!(
                            "bytes={}, chars={}",
                            data.content.len(),
                            data.content.chars().count()
                        ),
                    );

                    return bridge_success_object(serde_json::to_value(data).unwrap_or_default());
                }
                Err(e) => {
                    self.debug_error("chapter", "open_chapter_failed", &e.to_string());
                    return bridge_error_object(
                        "error.io",
                        "CORE_ERROR",
                        &format!("读取章节失败: {}", e),
                    );
                }
            }
        }

        bridge_error_object("error.core_error", "CORE_ERROR", "Core not initialized")
    }

    pub(crate) fn save_chapter(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        content: QString,
        allow_empty_overwrite: bool,
    ) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        let text_str = content.to_string();
        let len = text_str.len();
        self.debug_log(
            "chapter",
            "save_chapter_start",
            &format!(
                "len={} allow_empty_overwrite={}",
                len, allow_empty_overwrite
            ),
        );
        if len == 0 && allow_empty_overwrite {
            self.debug_log(
                "chapter",
                "save_chapter_empty_allowed_user_clear",
                &format!("chapter_id={}", c),
            );
        }

        let result_obj = if let Some(api) = self.core_api() {
            match api.save_chapter_content_with_options(
                &p,
                &v,
                &c,
                &text_str,
                allow_empty_overwrite,
            ) {
                Ok(receipt) => {
                    self.debug_log("chapter", "save_chapter_success", &format!("len={}", len));
                    self.current_save_status = "已保存".to_string();
                    // Issue #982：只更新本章正文条目，不重建整个工作区索引。
                    let title = self.chapter_title_from_tree(&p, &v, &c).unwrap_or_default();
                    let entry = writer_core::search::extractor::extract_chapter_body_entry(
                        &p, &v, &c, &title, &text_str,
                    );
                    self.search_bridge().apply_updates(vec![
                        writer_core::search::SearchIndexUpdate {
                            action: writer_core::search::SearchIndexAction::Upsert,
                            object_id: entry.object_id.clone(),
                            scope: entry.scope,
                            title: entry.title.clone(),
                            body: entry.body.clone(),
                            target: Some(entry.target.clone()),
                        },
                    ]);
                    // 正文保存不触发 workspace_state_changed，避免 reload_tree 刷新整棵树。
                    // 保存只改变章节内容，不改变工作区结构（项目/卷/章节增删改）。
                    serde_to_qjson_object(serde_json::json!({
                        "success": true,
                        "data": serde_json::to_value(receipt).unwrap_or_default(),

                        "changedEntities": ["ChapterContent"]
                    }))
                }
                Err(e) => {
                    self.debug_error("chapter", "save_chapter_failed", &format!("error={}", e));
                    if is_empty_overwrite_blocked(&e) {
                        self.debug_error(
                            "chapter",
                            "blocked_empty_overwrite",
                            &format!("chapter_id={} len={}", c, len),
                        );
                        let msg = blocked_empty_overwrite_user_message();
                        self.current_save_status = msg.to_string();
                        self.set_error("error.empty_overwrite_blocked");
                        serde_to_qjson_object(serde_json::json!({
                            "success": false,
                            "errorCode": blocked_empty_overwrite_error_code(),
                            "messageKey": "error.empty_overwrite_blocked",
                            "messageArgs": {},
                            "rawError": e.to_string(),
                            "changedEntities": []
                        }))
                    } else {
                        self.current_save_status = "保存失败".to_string();
                        serde_to_qjson_object(serde_json::json!({
                            "success": false,
                            "errorCode": "CORE_ERROR",
                            "messageKey": "error.core_error",
                            "messageArgs": {},
                            "rawError": e.to_string(),
                            "changedEntities": []
                        }))
                    }
                }
            }
        } else {
            self.debug_error("chapter", "save_chapter_failed", "core_not_initialized");
            self.current_save_status = "保存失败".to_string();
            bridge_error_object("error.core_error", "CORE_ERROR", "Core not initialized")
        };

        result_obj
    }

    /// 保存章节正文并携带调用方 base 版本。
    ///
    /// QML 把上次打开/保存得到的 generation/revision/sha256 一并传回；
    /// Core 发现磁盘版本已前进时返回 `CHAPTER_VERSION_CONFLICT`，不发布正文，
    /// 编辑器保留当前 buffer，避免旧文本覆盖新版本。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn save_chapter_versioned(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        content: QString,
        allow_empty_overwrite: bool,
        expected_generation: QString,
        expected_revision: i64,
        expected_content_sha256: QString,
        base_content: QString,
    ) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        let text_str = content.to_string();
        let len = text_str.len();
        let expected = writer_core::api::ChapterVersionDto {
            object_generation: expected_generation.to_string(),
            revision: u64::try_from(expected_revision).unwrap_or(0),
            content_sha256: expected_content_sha256.to_string(),
        };
        self.debug_log(
            "chapter",
            "save_chapter_versioned_start",
            &format!(
                "len={} expected_revision={} expected_sha256={}",
                len, expected.revision, expected.content_sha256
            ),
        );
        let Some(api) = self.core_api() else {
            self.debug_error(
                "chapter",
                "save_chapter_versioned_failed",
                "core_not_initialized",
            );
            self.current_save_status = "保存失败".to_string();
            return bridge_error_object("error.core_error", "CORE_ERROR", "Core not initialized");
        };

        let has_expected = !expected.object_generation.is_empty()
            || expected.revision > 0
            || !expected.content_sha256.is_empty();
        let expected = has_expected.then_some(expected);
        let base = (!base_content.is_empty()).then(|| base_content.to_string());
        let save_result = if allow_empty_overwrite {
            api.save_chapter_content_versioned_allow_empty(&p, &v, &c, &text_str, expected, base)
        } else {
            api.save_chapter_content_versioned(&p, &v, &c, &text_str, expected, base)
        };
        match save_result {
            Ok(receipt) => {
                self.debug_log(
                    "chapter",
                    "save_chapter_versioned_success",
                    &format!("len={} revision={}", len, receipt.version.revision),
                );
                let value = Self::chapter_save_success_value(&receipt);
                self.apply_chapter_save_success(&p, &v, &c, &text_str);
                serde_to_qjson_object(value)
            }
            Err(error) => {
                let value = self.record_chapter_save_failure(
                    &c,
                    len,
                    error.code(),
                    error.message_key(),
                    serde_json::to_value(error.params()).unwrap_or_else(|_| serde_json::json!({})),
                    &error.to_string(),
                );
                serde_to_qjson_object(value)
            }
        }
    }

    /// 保存成功后的统一副作用：状态栏文案 + 仅本章正文的增量搜索索引。
    ///
    /// 同步保存（旧入口）与 Issue #1059 的后台队列回执都走这里，保证两条路径
    /// 的 current_save_status / changedEntities 语义一致。
    pub(crate) fn apply_chapter_save_success(
        &mut self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        text: &str,
    ) {
        self.current_save_status = "已保存".to_string();
        // Issue #982：只更新本章正文条目，不重建整个工作区索引。
        let title = self
            .chapter_title_from_tree(project_id, volume_id, chapter_id)
            .unwrap_or_default();
        let entry = writer_core::search::extractor::extract_chapter_body_entry(
            project_id, volume_id, chapter_id, &title, text,
        );
        self.search_bridge()
            .apply_updates(vec![writer_core::search::SearchIndexUpdate {
                action: writer_core::search::SearchIndexAction::Upsert,
                object_id: entry.object_id.clone(),
                scope: entry.scope,
                title: entry.title.clone(),
                body: entry.body.clone(),
                target: Some(entry.target.clone()),
            }]);
    }

    /// 保存成功回执的 QML JSON 形状（纯数据，不产生副作用）。
    pub(crate) fn chapter_save_success_value(
        receipt: &writer_core::api::types::ChapterSaveReceiptDto,
    ) -> serde_json::Value {
        serde_json::json!({
            "success": true,
            "data": serde_json::to_value(receipt).unwrap_or_default(),
            "changedEntities": ["ChapterContent"]
        })
    }

    /// 保存失败的统一副作用：current_save_status / error_message。
    ///
    /// 冲突与失败都不覆盖编辑器 buffer，也不推进调用方 base；空正文拦截保持
    /// 旧提示。返回的 JSON 与同步保存路径完全同形。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_chapter_save_failure(
        &mut self,
        chapter_id: &str,
        len: usize,
        code: &str,
        message_key: &str,
        message_args: serde_json::Value,
        raw_error: &str,
    ) -> serde_json::Value {
        self.debug_error(
            "chapter",
            "save_chapter_failed",
            &format!("code={} error={}", code, raw_error),
        );
        if code == blocked_empty_overwrite_error_code() {
            self.debug_error(
                "chapter",
                "blocked_empty_overwrite",
                &format!("chapter_id={} len={}", chapter_id, len),
            );
            self.current_save_status = blocked_empty_overwrite_user_message().to_string();
            self.set_error("error.empty_overwrite_blocked");
        } else if code == "CHAPTER_VERSION_CONFLICT" {
            self.current_save_status = "保存冲突".to_string();
            self.set_error(message_key);
        } else {
            self.current_save_status = "保存失败".to_string();
            self.set_error(message_key);
        }
        Self::chapter_save_error_value(code, message_key, message_args, raw_error)
    }

    /// 保存失败回执的 QML JSON 形状（纯数据，不产生副作用）。
    pub(crate) fn chapter_save_error_value(
        code: &str,
        message_key: &str,
        message_args: serde_json::Value,
        raw_error: &str,
    ) -> serde_json::Value {
        if code == blocked_empty_overwrite_error_code() {
            serde_json::json!({
                "success": false,
                "errorCode": code,
                "messageKey": "error.empty_overwrite_blocked",
                "messageArgs": {},
                "rawError": raw_error,
                "changedEntities": []
            })
        } else {
            serde_json::json!({
                "success": false,
                "errorCode": code,
                "messageKey": message_key,
                "messageArgs": message_args,
                "rawError": raw_error,
                "changedEntities": []
            })
        }
    }

    /// 只读章节版本探针，供同步后先比较再决定是否重载正文。
    pub(crate) fn read_chapter_version(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        match self.core_api() {
            Some(api) => match api.read_chapter_version(&p, &v, &c) {
                Ok(version) => bridge_success_object(serde_json::json!({
                    "projectId": p,
                    "volumeId": v,
                    "chapterId": c,
                    "version": serde_json::to_value(version).unwrap_or_default()
                })),
                Err(error) => {
                    bridge_error_object(error.message_key(), error.code(), &error.to_string())
                }
            },
            None => bridge_error_object("error.core_error", "CORE_ERROR", "Core not initialized"),
        }
    }

    pub(crate) fn clear_chapter_content(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        self.debug_log(
            "chapter",
            "clear_chapter_content_start",
            &format!("chapter_id={}", c),
        );

        if let Some(api) = self.core_api() {
            let result = api.clear_chapter_content(&p, &v, &c);
            let envelope = match result {
                Ok(data) => writer_core::api::ResultEnvelope::success_with_changes(
                    data,
                    Vec::new(),
                    vec![writer_core::api::ChangedEntityDto {
                        entity_type: "ChapterCleared".to_string(),
                        entity_id: Some(c.clone()),
                    }],
                ),
                Err(error) => writer_core::api::ResultEnvelope::<
                    writer_core::api::types::ChapterSaveReceiptDto,
                >::error(error),
            };
            if envelope.success {
                self.debug_log(
                    "chapter",
                    "clear_chapter_content_success",
                    &format!("chapter_id={}", c),
                );
                self.current_save_status = "已清空".to_string();
            } else {
                let message_key = envelope
                    .message_key
                    .as_deref()
                    .unwrap_or("error.core_error");
                let raw_err = envelope.raw_error.as_deref().unwrap_or("未知错误");
                self.debug_error("chapter", "clear_chapter_content_failed", raw_err);
                self.current_save_status = "清空失败".to_string();
                self.set_error(message_key);
            }
            let value = serde_json::to_value(envelope.into_value_envelope()).unwrap_or_else(
                |_| serde_json::json!({"success": false, "errorCode": "JSON_ERROR"}),
            );
            serde_to_qjson_object(value)
        } else {
            self.debug_error(
                "chapter",
                "clear_chapter_content_failed",
                "core_not_initialized",
            );
            self.current_save_status = "清空失败".to_string();
            self.set_error("error.core_error");
            bridge_error_object("error.core_error", "CORE_ERROR", "Core not initialized")
        }
    }
}
