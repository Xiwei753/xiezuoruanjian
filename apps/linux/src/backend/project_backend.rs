use super::*;

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct ProjectBackend {
    base: qt_base_class!(trait QObject),
    projects_reloaded: qt_signal!(),
    #[allow(non_snake_case)]
    projectsReloaded: qt_signal!(),
    selected_item_changed: qt_signal!(),
    workspace_content_changed: qt_signal!(),
    refresh_app_state_json: qt_method!(fn(&mut self) -> QString),
    refresh_tree_model_json: qt_method!(fn(&mut self) -> QString),
    get_tree_model_json: qt_method!(fn(&self) -> QString),
    get_tree_model: qt_method!(fn(&self) -> QJsonArray),
    create_project_json: qt_method!(fn(&mut self, title: QString, action_id: QString) -> QString),
    create_volume_json: qt_method!(fn(&mut self, project_id: QString, title: QString, action_id: QString) -> QString),
    create_chapter_json: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, title: QString, action_id: QString) -> QString),
    select_tree_item_json: qt_method!(fn(&mut self, item_type: QString, project_id: QString, volume_id: QString, chapter_id: QString, action_id: QString) -> QString),
    delete_project_json: qt_method!(fn(&mut self, project_id: QString, action_id: QString) -> QString),
    delete_volume_json: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, action_id: QString) -> QString),
    delete_chapter_json: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, action_id: QString) -> QString),
    create_new_volume: qt_method!(fn(&mut self, project_id: QString, title: QString)),
    create_new_chapter: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, title: QString)),
    rename_project: qt_method!(fn(&mut self, project_id: QString, new_title: QString)),
    delete_project: qt_method!(fn(&mut self, project_id: QString)),
    reorder_projects: qt_method!(fn(&mut self, ordered_ids_joined: QString)),
    rename_volume: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, new_title: QString)),
    delete_volume: qt_method!(fn(&mut self, project_id: QString, volume_id: QString)),
    reorder_volumes: qt_method!(fn(&mut self, project_id: QString, ordered_ids_joined: QString)),
    rename_chapter: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, new_title: QString)),
    delete_chapter: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString)),
    reorder_chapters: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, ordered_ids_joined: QString)),
    select_project: qt_method!(fn(&mut self, project_id: QString)),
    select_volume: qt_method!(fn(&mut self, project_id: QString, volume_id: QString)),
    select_chapter: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString)),
    app: QPointer<AppBackend>,
}

impl ProjectBackend {
    pub fn new(app: QPointer<AppBackend>) -> Self { Self { app, ..Default::default() } }
    fn with_app<R>(&self, default: R, f: impl FnOnce(&AppBackend) -> R) -> R { self.app.as_pinned().map(|app| f(&app.borrow())).unwrap_or(default) }
    fn with_app_mut<R>(&mut self, default: R, f: impl FnOnce(&mut AppBackend) -> R) -> R { self.app.as_pinned().map(|app| f(&mut app.borrow_mut())).unwrap_or(default) }
    fn emit_changed(&mut self) { self.projects_reloaded(); self.projectsReloaded(); self.workspace_content_changed(); self.selected_item_changed(); }
    fn refresh_app_state_json(&mut self) -> QString { self.with_app_mut("{}".into(), |app| app.refresh_app_state_json()) }
    fn refresh_tree_model_json(&mut self) -> QString { let out = self.with_app_mut("[]".into(), |app| app.refresh_tree_model_json()); self.emit_changed(); out }
    fn get_tree_model_json(&self) -> QString { self.with_app("[]".into(), |app| app.get_tree_model_json()) }
    fn get_tree_model(&self) -> QJsonArray { self.with_app(QJsonArray::default(), |app| app.get_tree_model()) }
    fn create_project_json(&mut self, title: QString, action_id: QString) -> QString { let out = self.with_app_mut("{}".into(), |app| app.create_project_json(title, action_id)); self.emit_changed(); out }
    fn create_volume_json(&mut self, project_id: QString, title: QString, action_id: QString) -> QString { let out = self.with_app_mut("{}".into(), |app| app.create_volume_json(project_id, title, action_id)); self.emit_changed(); out }
    fn create_chapter_json(&mut self, project_id: QString, volume_id: QString, title: QString, action_id: QString) -> QString { let out = self.with_app_mut("{}".into(), |app| app.create_chapter_json(project_id, volume_id, title, action_id)); self.emit_changed(); out }
    fn select_tree_item_json(&mut self, item_type: QString, project_id: QString, volume_id: QString, chapter_id: QString, action_id: QString) -> QString { let out = self.with_app_mut("{}".into(), |app| app.select_tree_item_json(item_type, project_id, volume_id, chapter_id, action_id)); self.selected_item_changed(); out }
    fn delete_project_json(&mut self, project_id: QString, action_id: QString) -> QString { let out = self.with_app_mut("{}".into(), |app| app.delete_project_json(project_id, action_id)); self.emit_changed(); out }
    fn delete_volume_json(&mut self, project_id: QString, volume_id: QString, action_id: QString) -> QString { let out = self.with_app_mut("{}".into(), |app| app.delete_volume_json(project_id, volume_id, action_id)); self.emit_changed(); out }
    fn delete_chapter_json(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, action_id: QString) -> QString { let out = self.with_app_mut("{}".into(), |app| app.delete_chapter_json(project_id, volume_id, chapter_id, action_id)); self.emit_changed(); out }
    fn create_new_volume(&mut self, project_id: QString, title: QString) { self.with_app_mut((), |app| app.create_new_volume(project_id, title)); self.emit_changed(); }
    fn create_new_chapter(&mut self, project_id: QString, volume_id: QString, title: QString) { self.with_app_mut((), |app| app.create_new_chapter(project_id, volume_id, title)); self.emit_changed(); }
    fn rename_project(&mut self, project_id: QString, new_title: QString) { self.with_app_mut((), |app| app.rename_project(project_id, new_title)); self.emit_changed(); }
    fn delete_project(&mut self, project_id: QString) { self.with_app_mut((), |app| app.delete_project(project_id)); self.emit_changed(); }
    fn reorder_projects(&mut self, ordered_ids_joined: QString) { self.with_app_mut((), |app| app.reorder_projects(ordered_ids_joined)); self.emit_changed(); }
    fn rename_volume(&mut self, project_id: QString, volume_id: QString, new_title: QString) { self.with_app_mut((), |app| app.rename_volume(project_id, volume_id, new_title)); self.emit_changed(); }
    fn delete_volume(&mut self, project_id: QString, volume_id: QString) { self.with_app_mut((), |app| app.delete_volume(project_id, volume_id)); self.emit_changed(); }
    fn reorder_volumes(&mut self, project_id: QString, ordered_ids_joined: QString) { self.with_app_mut((), |app| app.reorder_volumes(project_id, ordered_ids_joined)); self.emit_changed(); }
    fn rename_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, new_title: QString) { self.with_app_mut((), |app| app.rename_chapter(project_id, volume_id, chapter_id, new_title)); self.emit_changed(); }
    fn delete_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) { self.with_app_mut((), |app| app.delete_chapter(project_id, volume_id, chapter_id)); self.emit_changed(); }
    fn reorder_chapters(&mut self, project_id: QString, volume_id: QString, ordered_ids_joined: QString) { self.with_app_mut((), |app| app.reorder_chapters(project_id, volume_id, ordered_ids_joined)); self.emit_changed(); }
    fn select_project(&mut self, project_id: QString) { self.with_app_mut((), |app| app.select_project(project_id)); self.selected_item_changed(); }
    fn select_volume(&mut self, project_id: QString, volume_id: QString) { self.with_app_mut((), |app| app.select_volume(project_id, volume_id)); self.selected_item_changed(); }
    fn select_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) { self.with_app_mut((), |app| app.select_chapter(project_id, volume_id, chapter_id)); self.selected_item_changed(); }
}

impl AppBackend {
// Included inside impl AppBackend from app_backend.rs.
// Deprecated compatibility methods for this Linux backend domain.

// AppBackend::reconcile_selection_after_tree_reload
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

        if let (Some(project_id), Some(volume_id)) = (self.selected_project_id.clone(), self.selected_volume_id.clone()) {
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
            let chapters = api.list_chapters(&project_id, &volume_id).unwrap_or_default();
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

// AppBackend::trigger_projects_reloaded
    pub(crate) fn trigger_projects_reloaded(&mut self) {
        self.projects_reloaded();
        self.projectsReloaded();
    }

// AppBackend::reload_tree
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
        self.debug_log("tree", "reload_tree", &format!("before_count={}, after_count={}", before_count, after_count));
    }

// AppBackend::build_tree_model_json
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

// AppBackend::get_tree_model_json
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

// AppBackend::refresh_tree_model_json
    pub(crate) fn refresh_tree_model_json(&mut self) -> QString {
        self.reload_tree();
        self.get_tree_model_json()
    }

// AppBackend::refresh_app_state_json
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

// AppBackend::create_project_json
    pub(crate) fn create_project_json(&mut self, title: QString, _action_id: QString) -> QString {
        let title_str = title.to_string();
        let build_err = |msg: &str| -> String {
            serde_json::json!({
                "success": false,
                "errorCode": "PROJECT_CREATION_FAILED",
                "userMessage": msg,
                "rawError": msg,
                "changedEntities": []
            }).to_string()
        };

        if title_str.trim().is_empty() {
            let msg = "作品名不能为空";
            self.set_error(msg);
            return build_err(msg).into();
        }

        if !self.current_has_workspace || self.current_workspace.is_empty() {
            let msg = "未打开工作区，无法创建作品。请先新建或打开一个工作区。";
            self.set_error(msg);
            return build_err(msg).into();
        }

        if let Some(api) = self.core_api() {
            match api.create_project(&title_str) {
                Ok(proj) => {
                    self.selected_project_id = Some(proj.id.clone());
                    self.selected_item_changed();
                    self.selected_volume_id = None;
                    self.selected_chapter_id = None;

                    let default_volume_id = {
                        if let Ok(volumes) = api.list_volumes(&proj.id) {
                            volumes.first().map(|v| v.id.clone())
                        } else {
                            None
                        }
                    };
                    if let Some(ref vol_id) = default_volume_id {
                        self.selected_volume_id = Some(vol_id.clone());
                    }

                    self.reload_tree();
                    self.trigger_projects_reloaded();

                    let state_val: serde_json::Value = serde_json::from_str(&self.refresh_app_state_json().to_string()).unwrap_or_default();

                    let final_res = serde_json::json!({
                        "success": true,
                        "data": { "project": proj },
                        "message": format!("作品「{}」创建成功", proj.title),
                        "state": state_val,
                        "changedEntities": ["ProjectList", "WorkspaceTree"]
                    });
                    final_res.to_string().into()
                }
                Err(e) => {
                    let err_display = format!("{}", e);
                    let msg = format!("创建作品失败: {}", e);
                    self.set_error(&msg);
                    serde_json::json!({
                        "success": false,
                        "errorCode": "CORE_ERROR",
                        "userMessage": msg,
                        "rawError": err_display,
                        "changedEntities": []
                    }).to_string().into()
                }
            }
        } else {
            let msg = "核心模块未初始化";
            self.set_error(msg);
            build_err(msg).into()
        }
    }

// AppBackend::create_volume_json
    pub(crate) fn create_volume_json(&mut self, project_id: QString, title: QString, action_id: QString) -> QString {
        let action_id_str = action_id.to_string();
        self.debug_log(
            "volume",
            "create_volume_start",
            &format!("[actionId={}] project_id={}, title={}", action_id_str, project_id.to_string(), title.to_string())
        );
        let err_before = self.current_error_message.clone();
        self.create_new_volume(project_id.clone(), title.clone());
        let success = self.current_error_message == err_before;
        if success {
            self.debug_log("volume", "create_volume_success", &format!("[actionId={}] created successfully", action_id_str));
        } else {
            self.debug_error("volume", "create_volume_failed", &format!("[actionId={}] error: {}", action_id_str, self.current_error_message));
        }
        let final_res = serde_json::json!({
            "success": true,
            "message": "创建卷成功",
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

// AppBackend::create_chapter_json
    pub(crate) fn create_chapter_json(&mut self, project_id: QString, volume_id: QString, title: QString, action_id: QString) -> QString {
        let action_id_str = action_id.to_string();
        self.debug_log(
            "chapter",
            "create_chapter_start",
            &format!("[actionId={}] project_id={}, volume_id={}, title={}", action_id_str, project_id.to_string(), volume_id.to_string(), title.to_string())
        );
        let err_before = self.current_error_message.clone();
        self.create_new_chapter(project_id.clone(), volume_id.clone(), title.clone());
        let success = self.current_error_message == err_before;
        if success {
            self.debug_log("chapter", "create_chapter_success", &format!("[actionId={}] created successfully", action_id_str));
        } else {
            self.debug_error("chapter", "create_chapter_failed", &format!("[actionId={}] error: {}", action_id_str, self.current_error_message));
        }
        let final_res = serde_json::json!({
            "success": true,
            "message": "创建章节成功",
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

// AppBackend::select_tree_item_json
    pub(crate) fn select_tree_item_json(&mut self, item_type: QString, project_id: QString, volume_id: QString, chapter_id: QString, action_id: QString) -> QString {
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
            )
        );
        if t == "project" {
            self.select_project(project_id);
        } else if t == "volume" {
            self.select_volume(project_id, volume_id);
        } else if t == "chapter" {
            self.select_chapter(project_id, volume_id, chapter_id);
        }
        self.debug_log("tree", "select_item_success", &format!("[actionId={}] selection completed", action_id_str));
        let final_res = serde_json::json!({
            "success": true,
            "message": "选择成功",
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

// AppBackend::delete_project_json
    pub(crate) fn delete_project_json(&mut self, project_id: QString, action_id: QString) -> QString {
        let action_id_str = action_id.to_string();
        self.debug_log(
            "project",
            "delete_project_start",
            &format!("[actionId={}] project_id={}", action_id_str, project_id.to_string())
        );
        self.current_error_message = "".into();
        self.delete_project(project_id);
        let success = self.current_error_message.is_empty();
        let msg = if success {
            self.debug_log("project", "delete_project_success", &format!("[actionId={}] project deleted successfully", action_id_str));
            "删除成功".to_string()
        } else {
            let err = self.current_error_message.clone();
            self.debug_error("project", "delete_project_failed", &format!("[actionId={}] error: {}", action_id_str, err));
            err
        };
        let final_res = serde_json::json!({
            "success": success,
            "message": msg,
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

// AppBackend::delete_volume_json
    pub(crate) fn delete_volume_json(&mut self, project_id: QString, volume_id: QString, action_id: QString) -> QString {
        let action_id_str = action_id.to_string();
        self.debug_log(
            "volume",
            "delete_volume_start",
            &format!("[actionId={}] project_id={}, volume_id={}", action_id_str, project_id.to_string(), volume_id.to_string())
        );
        self.current_error_message = "".into();
        self.delete_volume(project_id, volume_id);
        let success = self.current_error_message.is_empty();
        let msg = if success {
            self.debug_log("volume", "delete_volume_success", &format!("[actionId={}] volume deleted successfully", action_id_str));
            "删除成功".to_string()
        } else {
            let err = self.current_error_message.clone();
            self.debug_error("volume", "delete_volume_failed", &format!("[actionId={}] error: {}", action_id_str, err));
            err
        };
        let final_res = serde_json::json!({
            "success": success,
            "message": msg,
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

// AppBackend::delete_chapter_json
    pub(crate) fn delete_chapter_json(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, action_id: QString) -> QString {
        let action_id_str = action_id.to_string();
        self.debug_log(
            "chapter",
            "delete_chapter_start",
            &format!("[actionId={}] project_id={}, volume_id={}, chapter_id={}", action_id_str, project_id.to_string(), volume_id.to_string(), chapter_id.to_string())
        );
        self.current_error_message = "".into();
        self.delete_chapter(project_id, volume_id, chapter_id);
        let success = self.current_error_message.is_empty();
        let msg = if success {
            self.debug_log("chapter", "delete_chapter_success", &format!("[actionId={}] chapter deleted successfully", action_id_str));
            "删除成功".to_string()
        } else {
            let err = self.current_error_message.clone();
            self.debug_error("chapter", "delete_chapter_failed", &format!("[actionId={}] error: {}", action_id_str, err));
            err
        };
        let final_res = serde_json::json!({
            "success": success,
            "message": msg,
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

// AppBackend::get_tree_model
    pub(crate) fn get_tree_model(&self) -> QJsonArray {
        self.cached_tree.clone()
    }

// AppBackend::create_new_volume
    pub(crate) fn create_new_volume(&mut self, project_id: QString, title: QString) {
        if let Some(api) = self.core_api() {
            let result = api.create_volume(&project_id.to_string(), &title.to_string());
            match result {
                Ok(vol) => {
                    self.selected_project_id = Some(project_id.to_string());
                    self.selected_volume_id = Some(vol.id.clone());
                    self.selected_item_changed();
                    self.selected_chapter_id = None;
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("创建分卷失败: {}", e)),
            }
        }
    }

// AppBackend::create_new_chapter
    pub(crate) fn create_new_chapter(&mut self, project_id: QString, volume_id: QString, title: QString) {
        if let Some(api) = self.core_api() {
            let result = api.create_chapter(
                &project_id.to_string(),
                &volume_id.to_string(),
                &title.to_string(),
            );
            match result {
                Ok(chap) => {
                    self.selected_project_id = Some(project_id.to_string());
                    self.selected_volume_id = Some(volume_id.to_string());
                    self.selected_chapter_id = Some(chap.id.clone());
                    self.selected_item_changed();
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("创建章节失败: {}", e)),
            }
        }
    }

// AppBackend::rename_project
    pub(crate) fn rename_project(&mut self, project_id: QString, new_title: QString) {
        if let Some(api) = self.core_api() {
            let result = api.rename_project(&project_id.to_string(), &new_title.to_string());
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重命名作品失败: {}", e)),
            }
        }
    }

// AppBackend::delete_project
    pub(crate) fn delete_project(&mut self, project_id: QString) {
        if let Some(api) = self.core_api() {
            let result = api.delete_project(&project_id.to_string());
            match result {
                Ok(_) => {
                    if self.selected_project_id.as_deref() == Some(&project_id.to_string()) {
                        self.selected_project_id = None;
                        self.selected_volume_id = None;
                        self.clear_editor_state();
                    }

                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("删除作品失败: {}", e)),
            }
        }
    }

// AppBackend::reorder_projects
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

// AppBackend::rename_volume
    pub(crate) fn rename_volume(&mut self, project_id: QString, volume_id: QString, new_title: QString) {
        if let Some(api) = self.core_api() {
            let result = api.rename_volume(
                &project_id.to_string(),
                &volume_id.to_string(),
                &new_title.to_string(),
            );
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重命名分卷失败: {}", e)),
            }
        }
    }

// AppBackend::delete_volume
    pub(crate) fn delete_volume(&mut self, project_id: QString, volume_id: QString) {
        if let Some(api) = self.core_api() {
            let result = api.delete_volume(&project_id.to_string(), &volume_id.to_string());
            match result {
                Ok(_) => {
                    if self.selected_volume_id.as_deref() == Some(&volume_id.to_string()) {
                        self.selected_volume_id = None;
                        self.clear_editor_state();
                    }

                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("删除分卷失败: {}", e)),
            }
        }
    }

// AppBackend::reorder_volumes
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

// AppBackend::rename_chapter
    pub(crate) fn rename_chapter(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        new_title: QString,
    ) {
        if let Some(api) = self.core_api() {
            let result = api.rename_chapter(
                &project_id.to_string(),
                &volume_id.to_string(),
                &chapter_id.to_string(),
                &new_title.to_string(),
            );
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重命名章节失败: {}", e)),
            }
        }
    }

// AppBackend::delete_chapter
    pub(crate) fn delete_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) {
        if let Some(api) = self.core_api() {
            let result = api.delete_chapter(
                &project_id.to_string(),
                &volume_id.to_string(),
                &chapter_id.to_string(),
            );
            match result {
                Ok(_) => {
                    if self.selected_chapter_id.as_deref() == Some(&chapter_id.to_string()) {
                        self.clear_editor_state();
                    }

                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("删除章节失败: {}", e)),
            }
        }
    }

// AppBackend::reorder_chapters
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
            let result = api.reorder_chapters(&project_id.to_string(), &volume_id.to_string(), &ids);
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重排章节失败: {}", e)),
            }
        }
    }

// AppBackend::select_project
    pub(crate) fn select_project(&mut self, project_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = None;
        self.selected_chapter_id = None;
        self.selected_item_changed();
        self.chapter_path_changed();
    }

// AppBackend::select_volume
    pub(crate) fn select_volume(&mut self, project_id: QString, volume_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = Some(volume_id.to_string());
        self.selected_chapter_id = None;
        self.selected_item_changed();
        self.chapter_path_changed();
    }

// AppBackend::select_chapter
    pub(crate) fn select_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = Some(volume_id.to_string());
        self.selected_chapter_id = Some(chapter_id.to_string());
        self.selected_item_changed();
        self.chapter_path_changed();
    }

}
