// =============================================================================
// project_operations.rs — AppBackend 项目/分卷/章节 CRUD 操作
// =============================================================================


use super::*;
use qmetaobject::{QJsonArray, QJsonObject, QJsonValue, QString};
use writer_core::api::{ChangedEntityDto, ChapterMetaDto, ResultEnvelope, VolumeDto, WriterError};

impl AppBackend {
    pub(crate) fn reconcile_selection_after_tree_reload(&mut self) -> bool {
        let mut had_chapter_deleted = false;
        let Some(api) = self.core_api() else {
            return false;
        };

        if let Some(project_id) = self.selected_project_id.clone() {
            let projects = api.list_projects().unwrap_or_default();
            let project_exists = projects.iter().any(|p| p.id == project_id);
            if !project_exists {
                self.selected_project_id = None;
                self.selected_volume_id = None;
                self.clear_editor_state();
                self.selected_item_changed();
                self.chapter_path_changed();
                return false;
            }
        }

        if let (Some(project_id), Some(volume_id)) = (
            self.selected_project_id.clone(),
            self.selected_volume_id.clone(),
        ) {
            let volumes = api.list_volumes(&project_id).unwrap_or_default();
            let volume_exists = volumes.iter().any(|v| v.id == volume_id);
            if !volume_exists {
                self.selected_volume_id = None;
                self.clear_editor_state();
                self.selected_item_changed();
                self.chapter_path_changed();
                return false;
            }
        }

        if let (Some(project_id), Some(volume_id), Some(chapter_id)) = (
            self.selected_project_id.clone(),
            self.selected_volume_id.clone(),
            self.selected_chapter_id.clone(),
        ) {
            let chapters = api
                .list_chapters(&project_id, &volume_id)
                .unwrap_or_default();
            let chapter_exists = chapters.iter().any(|c| c.id == chapter_id);
            if !chapter_exists {
                self.clear_editor_state();
                had_chapter_deleted = true;
            }
        }

        self.selected_item_changed();
        self.chapter_path_changed();
        had_chapter_deleted
    }

    pub(crate) fn trigger_projects_reloaded(&mut self) {
        self.projects_reloaded();
        self.projectsReloaded();
    }

    pub(crate) fn reload_tree(&mut self) {
        let before_count = self.cached_tree.len();
        let mut list = QJsonArray::default();
        if let Some(core) = self.core_api() {
            if let Ok(projects) = core.list_projects() {
                for p in projects {
                    let mut p_map = QJsonObject::default();
                    p_map.insert(
                        "title".into(),
                        QJsonValue::from(QString::from(p.title.clone())),
                    );
                    p_map.insert("id".into(), QJsonValue::from(QString::from(p.id.clone())));
                    p_map.insert("type".into(), QJsonValue::from(QString::from("project")));
                    list.push(QJsonValue::from(p_map));

                    if let Ok(volumes) = core.list_volumes(&p.id) {
                        for v in volumes {
                            let mut v_map = QJsonObject::default();
                            v_map.insert(
                                "title".into(),
                                QJsonValue::from(QString::from(v.title.clone())),
                            );
                            v_map
                                .insert("id".into(), QJsonValue::from(QString::from(v.id.clone())));
                            v_map.insert(
                                "projectId".into(),
                                QJsonValue::from(QString::from(p.id.clone())),
                            );
                            v_map.insert("type".into(), QJsonValue::from(QString::from("volume")));
                            list.push(QJsonValue::from(v_map));

                            if let Ok(chapters) = core.list_chapters(&p.id, &v.id) {
                                for c in chapters {
                                    let mut c_map = QJsonObject::default();
                                    c_map.insert(
                                        "title".into(),
                                        QJsonValue::from(QString::from(c.title.clone())),
                                    );
                                    c_map.insert(
                                        "id".into(),
                                        QJsonValue::from(QString::from(c.id.clone())),
                                    );
                                    c_map.insert(
                                        "projectId".into(),
                                        QJsonValue::from(QString::from(p.id.clone())),
                                    );
                                    c_map.insert(
                                        "volumeId".into(),
                                        QJsonValue::from(QString::from(v.id.clone())),
                                    );
                                    c_map.insert(
                                        "type".into(),
                                        QJsonValue::from(QString::from("chapter")),
                                    );
                                    list.push(QJsonValue::from(c_map));
                                }
                            }
                        }
                    }
                }
            }
        }
        self.cached_tree = list;
        let after_count = self.cached_tree.len();
        self.debug_log(
            "tree",
            "reload_tree",
            &format!("before_count={}, after_count={}", before_count, after_count),
        );
    }

    pub(crate) fn build_tree_model_json(&self) -> serde_json::Value {
        use serde_json::json;
        let mut tree = Vec::new();
        if let Some(core) = self.core_api() {
            if let Ok(projects) = core.list_projects() {
                for p in &projects {
                    let mut p_map = serde_json::Map::new();
                    p_map.insert("title".into(), json!(p.title));
                    p_map.insert("id".into(), json!(p.id));
                    p_map.insert("type".into(), json!("project"));
                    p_map.insert("projectId".into(), json!(""));
                    p_map.insert("volumeId".into(), json!(""));
                    tree.push(serde_json::Value::Object(p_map));

                    if let Ok(volumes) = core.list_volumes(&p.id) {
                        for v in &volumes {
                            let mut v_map = serde_json::Map::new();
                            v_map.insert("title".into(), json!(v.title));
                            v_map.insert("id".into(), json!(v.id));
                            v_map.insert("projectId".into(), json!(p.id));
                            v_map.insert("volumeId".into(), json!(v.id));
                            v_map.insert("type".into(), json!("volume"));
                            tree.push(serde_json::Value::Object(v_map));

                            if let Ok(chapters) = core.list_chapters(&p.id, &v.id) {
                                for c in &chapters {
                                    let mut c_map = serde_json::Map::new();
                                    c_map.insert("title".into(), json!(c.title));
                                    c_map.insert("id".into(), json!(c.id));
                                    c_map.insert("projectId".into(), json!(p.id));
                                    c_map.insert("volumeId".into(), json!(v.id));
                                    c_map.insert("type".into(), json!("chapter"));
                                    tree.push(serde_json::Value::Object(c_map));
                                }
                            }
                        }
                    }
                }
            }
        }
        serde_json::Value::Array(tree)
    }

    pub(crate) fn get_tree_model_json(&self) -> QString {
        let items = self.build_tree_model_json();
        let count = match &items {
            serde_json::Value::Array(arr) => arr.len(),
            _ => 0,
        };
        let val = serde_json::json!({
            "success": true,
            "treeCount": count,
            "items": items
        });
        val.to_string().into()
    }

    pub(crate) fn refresh_tree_model_json(&mut self) -> QString {
        self.reload_tree();
        self.get_tree_model_json()
    }

    pub(crate) fn refresh_app_state_json(&mut self) -> QString {
        self.reload_tree();
        let tree_json = self.build_tree_model_json();
        let state = serde_json::json!({
            "hasWorkspace": self.current_has_workspace,
            "workspacePath": self.current_workspace,
            "saveStatus": self.current_save_status,
            "selected": {
                "projectId": self.selected_project_id.clone().unwrap_or_default(),
                "volumeId": self.selected_volume_id.clone().unwrap_or_default(),
                "chapterId": self.selected_chapter_id.clone().unwrap_or_default()
            },
            "tree": tree_json,
            "settings": {
                "fontSize": self.current_setting_font_size,
                "themeMode": self.setting_theme_mode().to_string()
            },
            "sync": {
                "status": self.current_sync_status
            }
        });
        state.to_string().into()
    }

    fn current_app_state_value(&self) -> serde_json::Value {
        serde_json::json!({
            "hasWorkspace": self.current_has_workspace,
            "workspacePath": self.current_workspace,
            "saveStatus": self.current_save_status,
            "selected": {
                "projectId": self.selected_project_id.clone().unwrap_or_default(),
                "volumeId": self.selected_volume_id.clone().unwrap_or_default(),
                "chapterId": self.selected_chapter_id.clone().unwrap_or_default()
            },
            "tree": self.build_tree_model_json(),
            "settings": {
                "fontSize": self.current_setting_font_size,
                "themeMode": self.setting_theme_mode().to_string()
            },
            "sync": {
                "status": self.current_sync_status
            }
        })
    }

    fn mutation_success_json(
        &mut self,
        data: serde_json::Value,
        user_message: &str,
        changed_entities: Vec<&str>,
    ) -> QString {
        serde_json::json!({
            "success": true,
            "data": data,
            "userMessage": user_message,
            "state": self.current_app_state_value(),
            "changedEntities": changed_entities
        })
        .to_string()
        .into()
    }

    fn mutation_error_json(&mut self, user_message: String, raw_error: String) -> QString {
        self.set_error(&user_message);
        serde_json::json!({
            "success": false,
            "errorCode": "CORE_ERROR",
            "userMessage": user_message,
            "rawError": raw_error,
            "state": self.current_app_state_value(),
            "changedEntities": []
        })
        .to_string()
        .into()
    }

    // -------------------------------------------------------------------------
    // Envelope 消费辅助函数
    // -------------------------------------------------------------------------
    fn core_envelope_to_result(
        &mut self,
        envelope_str: &str,
        on_success: impl FnOnce(&mut Self, &serde_json::Value),
    ) -> QString {
        match serde_json::from_str::<serde_json::Value>(envelope_str) {
            Ok(mut envelope) => {
                if envelope.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                    on_success(self, &envelope);
                    envelope["state"] = self.current_app_state_value();
                    envelope.to_string().into()
                } else {
                    let user_msg = envelope
                        .get("userMessage")
                        .and_then(|v| v.as_str())
                        .unwrap_or("操作失败")
                        .to_string();
                    let raw_error = envelope
                        .get("rawError")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&user_msg)
                        .to_string();
                    self.mutation_error_json(user_msg, raw_error)
                }
            }
            Err(e) => {
                let msg = format!("解析 envelope 失败: {}", e);
                self.mutation_error_json(msg.clone(), msg)
            }
        }
    }

    pub(crate) fn create_project_json(&mut self, title: QString, _action_id: QString) -> QString {
        let title_str = title.to_string();

        if title_str.trim().is_empty() {
            let msg = "作品名不能为空";
            return self.mutation_error_json(msg.to_string(), msg.to_string());
        }

        if !self.current_has_workspace || self.current_workspace.is_empty() {
            let msg = "未打开工作区，无法创建作品。请先新建或打开一个工作区。";
            return self.mutation_error_json(msg.to_string(), msg.to_string());
        }

        if let Some(api) = self.core_api() {
            let result = api.create_project(&title_str);
            let envelope = match result {
                Ok(project) => {
                    let project_id = project.id.clone();
                    ResultEnvelope::success_with_changes(
                        project,
                        Vec::new(),
                        vec![ChangedEntityDto { entity_type: "ProjectCreated".to_string(), entity_id: Some(project_id) }],
                    )
                }
                Err(error) => ResultEnvelope::<writer_core::api::types::ProjectDto>::error(error),
            }.to_json_string();
            self.core_envelope_to_result(&envelope, |app, value| {
                if let Some(proj_id) = value
                    .get("data")
                    .and_then(|d| d.get("id"))
                    .and_then(|v| v.as_str())
                {
                    app.selected_project_id = Some(proj_id.to_string());
                    app.selected_item_changed();
                    app.selected_volume_id = None;
                    app.selected_chapter_id = None;

                    let default_volume_id = {
                        if let Ok(volumes) = api.list_volumes(proj_id) {
                            volumes.first().map(|v| v.id.clone())
                        } else {
                            None
                        }
                    };
                    if let Some(ref vol_id) = default_volume_id {
                        app.selected_volume_id = Some(vol_id.clone());
                    }

                    app.reload_tree();
                    app.trigger_projects_reloaded();
                }
            })
        } else {
            let msg = "核心模块未初始化";
            self.mutation_error_json(msg.to_string(), msg.to_string())
        }
    }

    pub(crate) fn create_volume_json(
        &mut self,
        project_id: QString,
        title: QString,
        action_id: QString,
    ) -> QString {
        let action_id_str = action_id.to_string();
        self.debug_log(
            "volume",
            "create_volume_start",
            &format!(
                "[actionId={}] project_id={}, title={}",
                action_id_str,
                project_id.to_string(),
                title.to_string()
            ),
        );
        if let Some(api) = self.core_api() {
            let result = api.create_volume(&project_id.to_string(), &title.to_string());
            let envelope = match result {
                Ok(volume) => {
                    let volume_id = volume.id.clone();
                    ResultEnvelope::success_with_changes(
                        volume,
                        Vec::new(),
                        vec![ChangedEntityDto { entity_type: "VolumeCreated".to_string(), entity_id: Some(volume_id) }],
                    )
                }
                Err(error) => ResultEnvelope::<writer_core::api::types::VolumeDto>::error(error),
            }.to_json_string();
            self.core_envelope_to_result(&envelope, |app, value| {
                if let Some(vol_id) = value
                    .get("data")
                    .and_then(|d| d.get("id"))
                    .and_then(|v| v.as_str())
                {
                    app.selected_project_id = Some(project_id.to_string());
                    app.selected_volume_id = Some(vol_id.to_string());
                    app.selected_item_changed();
                    app.selected_chapter_id = None;
                    app.reload_tree();
                    app.trigger_projects_reloaded();
                }
                app.debug_log(
                    "volume",
                    "create_volume_success",
                    &format!("[actionId={}] volume_id", action_id_str),
                );
            })
        } else {
            let msg = "核心模块未初始化";
            self.mutation_error_json(msg.to_string(), msg.to_string())
        }
    }

    pub(crate) fn create_chapter_json(
        &mut self,
        project_id: QString,
        volume_id: QString,
        title: QString,
        action_id: QString,
    ) -> QString {
        let action_id_str = action_id.to_string();
        self.debug_log(
            "chapter",
            "create_chapter_start",
            &format!(
                "[actionId={}] project_id={}, volume_id={}, title={}",
                action_id_str,
                project_id.to_string(),
                volume_id.to_string(),
                title.to_string()
            ),
        );
        if let Some(api) = self.core_api() {
            let result = api.create_chapter(&project_id.to_string(), &volume_id.to_string(), &title.to_string());
            let envelope = match result {
                Ok(chapter) => {
                    let chapter_id = chapter.id.clone();
                    ResultEnvelope::success_with_changes(
                        chapter,
                        Vec::new(),
                        vec![ChangedEntityDto { entity_type: "ChapterCreated".to_string(), entity_id: Some(chapter_id) }],
                    )
                }
                Err(error) => ResultEnvelope::<writer_core::api::ChapterMetaDto>::error(error),
            }.to_json_string();
            self.core_envelope_to_result(&envelope, |app, value| {
                if let Some(chap_id) = value
                    .get("data")
                    .and_then(|d| d.get("id"))
                    .and_then(|v| v.as_str())
                {
                    app.selected_project_id = Some(project_id.to_string());
                    app.selected_volume_id = Some(volume_id.to_string());
                    app.selected_chapter_id = Some(chap_id.to_string());
                    app.selected_item_changed();
                    app.reload_tree();
                    app.trigger_projects_reloaded();
                }
                app.debug_log(
                    "chapter",
                    "create_chapter_success",
                    &format!("[actionId={}]", action_id_str),
                );
            })
        } else {
            let msg = "核心模块未初始化";
            self.mutation_error_json(msg.to_string(), msg.to_string())
        }
    }

    pub(crate) fn select_tree_item_json(
        &mut self,
        item_type: QString,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        action_id: QString,
    ) -> QString {
        let t = item_type.to_string();
        let action_id_str = action_id.to_string();
        self.debug_log(
            "tree",
            "select_item_start",
            &format!(
                "[actionId={}] type={}, project_id={}, volume_id={}, chapter_id={}",
                action_id_str,
                t,
                project_id.to_string(),
                volume_id.to_string(),
                chapter_id.to_string()
            ),
        );
        if t == "project" {
            self.select_project(project_id);
        } else if t == "volume" {
            self.select_volume(project_id, volume_id);
        } else if t == "chapter" {
            self.select_chapter(project_id, volume_id, chapter_id);
        }
        self.debug_log(
            "tree",
            "select_item_success",
            &format!("[actionId={}] selection completed", action_id_str),
        );
        let final_res = serde_json::json!({
            "success": true,
            "userMessage": "选择成功",
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

    pub(crate) fn delete_project_json(
        &mut self,
        project_id: QString,
        action_id: QString,
    ) -> QString {
        let project_id_str = project_id.to_string();
        let action_id_str = action_id.to_string();
        self.debug_log(
            "project",
            "delete_project_start",
            &format!("[actionId={}] project_id={}", action_id_str, project_id_str),
        );
        if let Some(api) = self.core_api() {
            let result = api.delete_project(&project_id_str);
            let envelope = match result {
                Ok(_) => ResultEnvelope::success_with_changes(
                    true,
                    Vec::new(),
                    vec![ChangedEntityDto { entity_type: "ProjectDeleted".to_string(), entity_id: Some(project_id_str.clone()) }],
                ),
                Err(error) => ResultEnvelope::<bool>::error(error),
            }.to_json_string();
            self.core_envelope_to_result(&envelope, |app, _value| {
                if app.selected_project_id.as_deref() == Some(&project_id_str) {
                    app.selected_project_id = None;
                    app.selected_volume_id = None;
                    app.clear_editor_state();
                }
                app.reload_tree();
                app.trigger_projects_reloaded();
                app.debug_log(
                    "project",
                    "delete_project_success",
                    &format!("[actionId={}] deleted", action_id_str),
                );
            })
        } else {
            let msg = "核心模块未初始化";
            self.mutation_error_json(msg.to_string(), msg.to_string())
        }
    }

    pub(crate) fn delete_volume_json(
        &mut self,
        project_id: QString,
        volume_id: QString,
        action_id: QString,
    ) -> QString {
        let project_id_str = project_id.to_string();
        let volume_id_str = volume_id.to_string();
        let action_id_str = action_id.to_string();
        self.debug_log(
            "volume",
            "delete_volume_start",
            &format!(
                "[actionId={}] project_id={}, volume_id={}",
                action_id_str, project_id_str, volume_id_str
            ),
        );
        if let Some(api) = self.core_api() {
            let result = api.delete_volume(&project_id_str, &volume_id_str);
            let envelope = match result {
                Ok(_) => ResultEnvelope::success_with_changes(
                    true,
                    Vec::new(),
                    vec![ChangedEntityDto { entity_type: "VolumeDeleted".to_string(), entity_id: Some(volume_id_str.clone()) }],
                ),
                Err(error) => ResultEnvelope::<bool>::error(error),
            }.to_json_string();
            self.core_envelope_to_result(&envelope, |app, _value| {
                if app.selected_volume_id.as_deref() == Some(&volume_id_str) {
                    app.selected_volume_id = None;
                    app.clear_editor_state();
                }
                app.reload_tree();
                app.trigger_projects_reloaded();
                app.debug_log(
                    "volume",
                    "delete_volume_success",
                    &format!("[actionId={}] deleted", action_id_str),
                );
            })
        } else {
            let msg = "核心模块未初始化";
            self.mutation_error_json(msg.to_string(), msg.to_string())
        }
    }

    pub(crate) fn delete_chapter_json(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        action_id: QString,
    ) -> QString {
        let project_id_str = project_id.to_string();
        let volume_id_str = volume_id.to_string();
        let chapter_id_str = chapter_id.to_string();
        let action_id_str = action_id.to_string();
        self.debug_log(
            "chapter",
            "delete_chapter_start",
            &format!(
                "[actionId={}] project_id={}, volume_id={}, chapter_id={}",
                action_id_str, project_id_str, volume_id_str, chapter_id_str
            ),
        );
        if let Some(api) = self.core_api() {
            let result = api.delete_chapter(&project_id_str, &volume_id_str, &chapter_id_str);
            let envelope = match result {
                Ok(_) => ResultEnvelope::success_with_changes(
                    true,
                    Vec::new(),
                    vec![ChangedEntityDto { entity_type: "ChapterDeleted".to_string(), entity_id: Some(chapter_id_str.clone()) }],
                ),
                Err(error) => ResultEnvelope::<bool>::error(error),
            }.to_json_string();
            self.core_envelope_to_result(&envelope, |app, _value| {
                if app.selected_chapter_id.as_deref() == Some(&chapter_id_str) {
                    app.clear_editor_state();
                }
                app.reload_tree();
                app.trigger_projects_reloaded();
                app.debug_log(
                    "chapter",
                    "delete_chapter_success",
                    &format!("[actionId={}] deleted", action_id_str),
                );
            })
        } else {
            let msg = "核心模块未初始化";
            self.mutation_error_json(msg.to_string(), msg.to_string())
        }
    }

    pub(crate) fn get_tree_model(&self) -> QJsonArray {
        self.cached_tree.clone()
    }

    pub(crate) fn create_new_volume(
        &mut self,
        project_id: QString,
        title: QString,
    ) -> Result<VolumeDto, WriterError> {
        let Some(api) = self.core_api() else {
            return Err(WriterError::InvalidWorkspace);
        };
        let project_id_str = project_id.to_string();
        let vol = api.create_volume(&project_id_str, &title.to_string())?;
        self.selected_project_id = Some(project_id_str);
        self.selected_volume_id = Some(vol.id.clone());
        self.selected_item_changed();
        self.selected_chapter_id = None;
        self.reload_tree();
        self.trigger_projects_reloaded();
        Ok(vol)
    }

    pub(crate) fn create_new_chapter(
        &mut self,
        project_id: QString,
        volume_id: QString,
        title: QString,
    ) -> Result<ChapterMetaDto, WriterError> {
        let Some(api) = self.core_api() else {
            return Err(WriterError::InvalidWorkspace);
        };
        let project_id_str = project_id.to_string();
        let volume_id_str = volume_id.to_string();
        let chap = api.create_chapter(&project_id_str, &volume_id_str, &title.to_string())?;
        self.selected_project_id = Some(project_id_str);
        self.selected_volume_id = Some(volume_id_str);
        self.selected_chapter_id = Some(chap.id.clone());
        self.selected_item_changed();
        self.reload_tree();
        self.trigger_projects_reloaded();
        Ok(chap)
    }

    pub(crate) fn rename_project_json(
        &mut self,
        project_id: QString,
        new_title: QString,
    ) -> QString {
        if let Some(api) = self.core_api() {
            let result = api.rename_project(&project_id.to_string(), &new_title.to_string());
            let envelope = match result {
                Ok(data) => ResultEnvelope::success_with_changes(
                    data,
                    Vec::new(),
                    vec![ChangedEntityDto { entity_type: "ProjectRenamed".to_string(), entity_id: Some(project_id.to_string()) }],
                ),
                Err(error) => ResultEnvelope::<writer_core::api::types::ProjectDto>::error(error),
            }.to_json_string();
            self.core_envelope_to_result(&envelope, |app, _value| {
                app.reload_tree();
                app.trigger_projects_reloaded();
            })
        } else {
            let raw_error = WriterError::InvalidWorkspace.to_string();
            self.mutation_error_json(format!("重命名作品失败: {}", raw_error), raw_error)
        }
    }

    pub(crate) fn delete_project(&mut self, project_id: QString) -> Result<bool, WriterError> {
        let Some(api) = self.core_api() else {
            return Err(WriterError::InvalidWorkspace);
        };
        let project_id_str = project_id.to_string();
        api.delete_project(&project_id_str)?;
        if self.selected_project_id.as_deref() == Some(&project_id_str) {
            self.selected_project_id = None;
            self.selected_volume_id = None;
            self.clear_editor_state();
        }
        self.reload_tree();
        self.trigger_projects_reloaded();
        Ok(true)
    }

    pub(crate) fn reorder_projects(&mut self, ordered_ids_joined: QString) {
        if let Some(api) = self.core_api() {
            let ordered_ids_str = ordered_ids_joined.to_string();
            let ids: Vec<String> = ordered_ids_str
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            let result = api.reorder_projects(&ids);
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重排作品失败: {}", e)),
            }
        }
    }

    pub(crate) fn rename_volume_json(
        &mut self,
        project_id: QString,
        volume_id: QString,
        new_title: QString,
    ) -> QString {
        if let Some(api) = self.core_api() {
            let result = api.rename_volume(&project_id.to_string(), &volume_id.to_string(), &new_title.to_string());
            let envelope = match result {
                Ok(data) => ResultEnvelope::success_with_changes(
                    data,
                    Vec::new(),
                    vec![ChangedEntityDto { entity_type: "VolumeRenamed".to_string(), entity_id: Some(volume_id.to_string()) }],
                ),
                Err(error) => ResultEnvelope::<writer_core::api::types::VolumeDto>::error(error),
            }.to_json_string();
            self.core_envelope_to_result(&envelope, |app, _value| {
                app.reload_tree();
                app.trigger_projects_reloaded();
            })
        } else {
            let raw_error = WriterError::InvalidWorkspace.to_string();
            self.mutation_error_json(format!("重命名分卷失败: {}", raw_error), raw_error)
        }
    }

    pub(crate) fn delete_volume(
        &mut self,
        project_id: QString,
        volume_id: QString,
    ) -> Result<bool, WriterError> {
        let Some(api) = self.core_api() else {
            return Err(WriterError::InvalidWorkspace);
        };
        let project_id_str = project_id.to_string();
        let volume_id_str = volume_id.to_string();
        api.delete_volume(&project_id_str, &volume_id_str)?;
        if self.selected_volume_id.as_deref() == Some(&volume_id_str) {
            self.selected_volume_id = None;
            self.clear_editor_state();
        }
        self.reload_tree();
        self.trigger_projects_reloaded();
        Ok(true)
    }

    pub(crate) fn reorder_volumes(&mut self, project_id: QString, ordered_ids_joined: QString) {
        if let Some(api) = self.core_api() {
            let ordered_ids_str = ordered_ids_joined.to_string();
            let ids: Vec<String> = ordered_ids_str
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            let result = api.reorder_volumes(&project_id.to_string(), &ids);
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重排分卷失败: {}", e)),
            }
        }
    }

    pub(crate) fn rename_chapter_json(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        new_title: QString,
    ) -> QString {
        if let Some(api) = self.core_api() {
            let result = api.rename_chapter(&project_id.to_string(), &volume_id.to_string(), &chapter_id.to_string(), &new_title.to_string());
            let envelope = match result {
                Ok(data) => ResultEnvelope::success_with_changes(
                    data,
                    Vec::new(),
                    vec![ChangedEntityDto { entity_type: "ChapterRenamed".to_string(), entity_id: Some(chapter_id.to_string()) }],
                ),
                Err(error) => ResultEnvelope::<writer_core::api::ChapterMetaDto>::error(error),
            }.to_json_string();
            self.core_envelope_to_result(&envelope, |app, _value| {
                app.reload_tree();
                app.trigger_projects_reloaded();
            })
        } else {
            let raw_error = WriterError::InvalidWorkspace.to_string();
            self.mutation_error_json(format!("重命名章节失败: {}", raw_error), raw_error)
        }
    }

    pub(crate) fn delete_chapter(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> Result<bool, WriterError> {
        let Some(api) = self.core_api() else {
            return Err(WriterError::InvalidWorkspace);
        };
        let project_id_str = project_id.to_string();
        let volume_id_str = volume_id.to_string();
        let chapter_id_str = chapter_id.to_string();
        api.delete_chapter(&project_id_str, &volume_id_str, &chapter_id_str)?;
        if self.selected_chapter_id.as_deref() == Some(&chapter_id_str) {
            self.clear_editor_state();
        }
        self.reload_tree();
        self.trigger_projects_reloaded();
        Ok(true)
    }

    pub(crate) fn reorder_chapters(
        &mut self,
        project_id: QString,
        volume_id: QString,
        ordered_ids_joined: QString,
    ) {
        if let Some(api) = self.core_api() {
            let ordered_ids_str = ordered_ids_joined.to_string();
            let ids: Vec<String> = ordered_ids_str
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            let result =
                api.reorder_chapters(&project_id.to_string(), &volume_id.to_string(), &ids);
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重排章节失败: {}", e)),
            }
        }
    }

    pub(crate) fn select_project(&mut self, project_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = None;
        self.selected_chapter_id = None;
        self.selected_item_changed();
        self.chapter_path_changed();
    }

    pub(crate) fn select_volume(&mut self, project_id: QString, volume_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = Some(volume_id.to_string());
        self.selected_chapter_id = None;
        self.selected_item_changed();
        self.chapter_path_changed();
    }

    pub(crate) fn select_chapter(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = Some(volume_id.to_string());
        self.selected_chapter_id = Some(chapter_id.to_string());
        self.selected_item_changed();
        self.chapter_path_changed();
    }
}
