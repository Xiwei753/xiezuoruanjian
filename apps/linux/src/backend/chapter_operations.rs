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
        self.debug_log("chapter", "get_chapter_content_start", &format!("project_id={}, volume_id={}, chapter_id={}", p, v, c));
        if let Some(api) = self.core_api() {
            match api.open_chapter(&p, &v, &c) {
                Ok(content) => {
                    self.debug_log("chapter", "get_chapter_content_success", &format!("len={}", content.content.len()));
                    return content.content.into();
                }
                Err(e) => {
                    self.debug_error("chapter", "get_chapter_content_failed", &format!("error={}", e));
                }
            }
        } else {
            self.debug_error("chapter", "get_chapter_content_failed", "core_not_initialized");
        }
        "".into()
    }

    pub(crate) fn open_chapter_json(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        if let Some(core) = self.core_api() {
            return match core.open_chapter(&p, &v, &c) {
                Ok(content) => serde_json::json!({
                    "success": true,
                    "content": content.content,
                    "title": content.meta.title,
                    "projectId": p,
                    "volumeId": v,
                    "chapterId": c,
                    "meta": content.meta,
                }).to_string().into(),
                Err(e) => serde_json::json!({
                    "success": false,
                    "code": "CORE_ERROR",
                    "error": format!("读取章节失败: {}", e),
                    "message": format!("读取章节失败: {}", e),
                }).to_string().into(),
            };
        }
        serde_json::json!({
            "success": false,
            "code": "INVALID_WORKSPACE",
            "error": "后端未初始化",
            "message": "后端未初始化",
        }).to_string().into()
    }

    pub(crate) fn open_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        self.debug_log("chapter", "open_chapter_start", &format!("project_id={}, volume_id={}, chapter_id={}", p, v, c));
        
        if let Some(core) = self.core_api() {
            match writing_bridge::open_chapter(&core, &p, &v, &c) {
                Ok(data) => {
                    self.selected_project_id = Some(p.clone());
                    self.selected_volume_id = Some(v.clone());
                    self.selected_chapter_id = Some(c.clone());
                    self.selected_item_changed();
                    self.chapter_path_changed();
                    
                    self.debug_log("chapter", "open_chapter_success", "len_loaded");
                    
                    let mut obj = serde_to_qjson_object(serde_json::to_value(data).unwrap_or_default());
                    obj.insert("success", serde_value_to_qjson(serde_json::Value::Bool(true)));
                    return obj;
                }
                Err(e) => {
                    self.debug_error("chapter", "open_chapter_failed", &e.to_string());
                    return bridge_error_object(&format!("读取章节失败: {}", e), "CORE_ERROR");
                }
            }
        }
        
        bridge_error_object("后端未初始化", "INVALID_WORKSPACE")
    }

    pub(crate) fn save_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, content: QString, allow_empty_overwrite: bool) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        let text_str = content.to_string();
        let len = text_str.len();
        self.debug_log("chapter", "save_chapter_start", &format!("len={} allow_empty_overwrite={}", len, allow_empty_overwrite));
        if len == 0 && allow_empty_overwrite {
            self.debug_log("chapter", "save_chapter_empty_allowed_user_clear", &format!("chapter_id={}", c));
        }
        
        let save_result = if let Some(core) = self.core_api() {
            Some(writing_bridge::save_chapter(&core, &p, &v, &c, &text_str, allow_empty_overwrite))
        } else {
            None
        };
        
        let result_obj = match save_result {
            Some(Ok(receipt)) => {
                self.debug_log("chapter", "save_chapter_success", &format!("len={}", len));
                self.current_save_status = "已保存".to_string();
                self.workspace_content_changed();
                self.flush_writing_stats();
                bridge_success_object(serde_json::to_value(receipt).unwrap())
            }
            Some(Err(e)) => {
                self.debug_error("chapter", "save_chapter_failed", &format!("error={}", e));
                if is_empty_overwrite_blocked(&e) {
                    self.debug_error("chapter", "blocked_empty_overwrite", &format!("chapter_id={} len={}", c, len));
                    let msg = blocked_empty_overwrite_user_message();
                    self.current_save_status = msg.to_string();
                    self.set_error(msg);
                    bridge_error_object(msg, blocked_empty_overwrite_error_code())
                } else {
                    self.current_save_status = "保存失败".to_string();
                    bridge_error_object(&format!("保存失败: {}", e), "CORE_ERROR")
                }
            }
            None => {
                self.debug_error("chapter", "save_chapter_failed", "core_not_initialized");
                bridge_error_object("后端未初始化", "INVALID_WORKSPACE")
            }
        };
        
        self.save_status_changed();
        result_obj
    }

    pub(crate) fn clear_chapter_content(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        self.debug_log("chapter", "clear_chapter_content_start", &format!("chapter_id={}", c));

        let clear_result = if let Some(core) = self.core_api() {
            Some(writing_bridge::clear_chapter_content(&core, &p, &v, &c))
        } else {
            None
        };

        match clear_result {
            Some(Ok(receipt)) => {
                self.debug_log("chapter", "clear_chapter_content_success", &format!("chapter_id={}", c));
                self.current_save_status = "已清空".to_string();
                self.save_status_changed();
                self.workspace_content_changed();
                bridge_success_object(serde_json::to_value(receipt).unwrap())
            }
            Some(Err(e)) => {
                let err_msg = format!("清空章节失败: {}", e);
                self.debug_error("chapter", "clear_chapter_content_failed", &err_msg);
                self.current_save_status = "清空失败".to_string();
                self.save_status_changed();
                self.set_error(&err_msg);
                bridge_error_object(&err_msg, "CORE_ERROR")
            }
            None => {
                self.debug_error("chapter", "clear_chapter_content_failed", "core_not_initialized");
                self.current_save_status = "清空失败".to_string();
                self.save_status_changed();
                self.set_error("清空章节失败: 后端未初始化");
                bridge_error_object("清空章节失败: 后端未初始化", "INVALID_WORKSPACE")
            }
        }
    }
}
