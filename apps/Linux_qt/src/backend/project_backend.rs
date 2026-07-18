// =============================================================================
// project_backend.rs — 作品、分卷及章节生命周期领域 QObject 后端适配层
// =============================================================================

use crate::backend::json_utils::qjson_object_from_json;
use super::*;
use crate::backend::AppRef;
use qmetaobject::QJsonObject;

#[path = "project_operations.rs"]
mod project_operations;

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct ProjectBackend {
    base: qt_base_class!(trait QObject),
    projects_reloaded: qt_signal!(),
    #[allow(non_snake_case)]
    projectsReloaded: qt_signal!(),
    selected_item_changed: qt_signal!(),
    workspace_content_changed: qt_signal!(),
    refresh_app_state: qt_method!(fn(&mut self) -> QJsonObject),
    refresh_tree_model_json: qt_method!(fn(&mut self) -> QString),
    get_tree_model_json: qt_method!(fn(&self) -> QString),
    get_tree_model: qt_method!(fn(&self) -> QJsonArray),
    get_project_summaries_json: qt_method!(fn(&self) -> QString),
    create_project: qt_method!(fn(&mut self, title: QString, action_id: QString) -> QJsonObject),
    create_volume: qt_method!(
        fn(&mut self, project_id: QString, title: QString, action_id: QString) -> QJsonObject
    ),
    create_chapter: qt_method!(
        fn(
            &mut self,
            project_id: QString,
            volume_id: QString,
            title: QString,
            action_id: QString,
        ) -> QJsonObject
    ),
    select_tree_item: qt_method!(
        fn(
            &mut self,
            item_type: QString,
            project_id: QString,
            volume_id: QString,
            chapter_id: QString,
            action_id: QString,
        ) -> QJsonObject
    ),
    delete_project_result:
        qt_method!(fn(&mut self, project_id: QString, action_id: QString) -> QJsonObject),
    delete_volume_result: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, action_id: QString) -> QJsonObject
    ),
    delete_chapter_result: qt_method!(
        fn(
            &mut self,
            project_id: QString,
            volume_id: QString,
            chapter_id: QString,
            action_id: QString,
        ) -> QJsonObject
    ),
    create_new_volume: qt_method!(fn(&mut self, project_id: QString, title: QString)),
    create_new_chapter:
        qt_method!(fn(&mut self, project_id: QString, volume_id: QString, title: QString)),
    rename_project:
        qt_method!(fn(&mut self, project_id: QString, new_title: QString) -> QJsonObject),
    delete_project: qt_method!(fn(&mut self, project_id: QString)),
    reorder_projects: qt_method!(fn(&mut self, ordered_ids_joined: QString)),
    rename_volume: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, new_title: QString) -> QJsonObject
    ),
    delete_volume: qt_method!(fn(&mut self, project_id: QString, volume_id: QString)),
    reorder_volumes: qt_method!(fn(&mut self, project_id: QString, ordered_ids_joined: QString)),
    rename_chapter: qt_method!(
        fn(
            &mut self,
            project_id: QString,
            volume_id: QString,
            chapter_id: QString,
            new_title: QString,
        ) -> QJsonObject
    ),
    delete_chapter:
        qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString)),
    reorder_chapters: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, ordered_ids_joined: QString)
    ),
    select_project: qt_method!(fn(&mut self, project_id: QString)),
    select_volume: qt_method!(fn(&mut self, project_id: QString, volume_id: QString)),
    select_chapter:
        qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString)),
    app: AppRef,
}

impl ProjectBackend {
    pub fn new(app: AppRef) -> Self {
        Self {
            app,
            ..Default::default()
        }
    }
    fn with_app<R>(&self, f: impl FnOnce(&AppBackend) -> R) -> Result<R, crate::backend::AppBorrowError> {
        self.app.with_app(f)
    }
    fn with_app_mut<R>(&self, f: impl FnOnce(&mut AppBackend) -> R) -> Result<R, crate::backend::AppBorrowError> {
        self.app.with_app_mut(f)
    }
    fn emit_changed(&mut self) {
        self.projects_reloaded();
        self.projectsReloaded();
        self.workspace_content_changed();
        self.selected_item_changed();
    }
    fn refresh_app_state(&mut self) -> QJsonObject {
        let res = self.with_app_mut(|app| app.refresh_app_state_json())
            .unwrap_or_else(|_| QString::from(crate::backend::json_utils::borrow_conflict_error_json()));
        qjson_object_from_json(&res.to_string())
    }
    fn refresh_tree_model_json(&mut self) -> QString {
        let result = self.with_app_mut(|app| app.refresh_tree_model_json());
        if result.is_ok() {
            self.emit_changed();
        }
        result.unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn get_tree_model_json(&self) -> QString {
        self.with_app(|app| app.get_tree_model_json()).unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn get_tree_model(&self) -> QJsonArray {
        match self.with_app(|app| app.get_tree_model()) {
            Ok(arr) => arr,
            Err(_) => crate::backend::json_utils::serde_to_qjson_array(
                serde_json::from_str(&crate::backend::json_utils::borrow_conflict_error_json())
                    .unwrap_or_else(|_| serde_json::json!([{"success": false, "errorCode": "BORROW_CONFLICT"}]))
            ),
        }
    }
    fn get_project_summaries_json(&self) -> QString {
        self.with_app(|app| app.get_project_summaries_json()).unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn create_project(&mut self, title: QString, action_id: QString) -> QJsonObject {
        let result = self.with_app_mut(|app| app.create_project_json(title, action_id));
        if result.is_ok() {
            self.emit_changed();
        }
        let out = result.unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into());
        qjson_object_from_json(&out.to_string())
    }
    fn create_volume(
        &mut self,
        project_id: QString,
        title: QString,
        action_id: QString,
    ) -> QJsonObject {
        let result = self.with_app_mut(|app| {
            app.create_volume_json(project_id, title, action_id)
        });
        if result.is_ok() {
            self.emit_changed();
        }
        let out = result.unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into());
        qjson_object_from_json(&out.to_string())
    }
    fn create_chapter(
        &mut self,
        project_id: QString,
        volume_id: QString,
        title: QString,
        action_id: QString,
    ) -> QJsonObject {
        let result = self.with_app_mut(|app| {
            app.create_chapter_json(project_id, volume_id, title, action_id)
        });
        if result.is_ok() {
            self.emit_changed();
        }
        let out = result.unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into());
        qjson_object_from_json(&out.to_string())
    }
    fn select_tree_item(
        &mut self,
        item_type: QString,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        _action_id: QString,
    ) -> QJsonObject {
        let item_type_str = item_type.to_string();
        let result = match item_type_str.as_str() {
            "project" => self.with_app_mut(|app| app.select_project(project_id)),
            "volume" => self.with_app_mut(|app| app.select_volume(project_id, volume_id)),
            "chapter" => self.with_app_mut(|app| {
                app.select_chapter(project_id, volume_id, chapter_id)
            }),
            _ => Ok(()),
        };
        if result.is_ok() {
            self.selected_item_changed();
            bridge_success_object(serde_json::json!({}))
        } else {
            qjson_object_from_json(&crate::backend::json_utils::borrow_conflict_error_json())
        }
    }
    fn delete_project_result(&mut self, project_id: QString, action_id: QString) -> QJsonObject {
        let result = self.with_app_mut(|app| {
            app.delete_project_json(project_id, action_id)
        });
        if result.is_ok() {
            self.emit_changed();
        }
        let out = result.unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into());
        qjson_object_from_json(&out.to_string())
    }
    fn delete_volume_result(
        &mut self,
        project_id: QString,
        volume_id: QString,
        action_id: QString,
    ) -> QJsonObject {
        let result = self.with_app_mut(|app| {
            app.delete_volume_json(project_id, volume_id, action_id)
        });
        if result.is_ok() {
            self.emit_changed();
        }
        let out = result.unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into());
        qjson_object_from_json(&out.to_string())
    }
    fn delete_chapter_result(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        action_id: QString,
    ) -> QJsonObject {
        let result = self.with_app_mut(|app| {
            app.delete_chapter_json(project_id, volume_id, chapter_id, action_id)
        });
        if result.is_ok() {
            self.emit_changed();
        }
        let out = result.unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into());
        qjson_object_from_json(&out.to_string())
    }
    fn create_new_volume(&mut self, project_id: QString, title: QString) {
        if self.with_app_mut(|app| {
            if let Err(e) = app.create_new_volume(project_id, title) {
                app.set_error(&format!("创建分卷失败: {}", e));
            }
        }).is_ok() {
            self.emit_changed();
        }
    }
    fn create_new_chapter(&mut self, project_id: QString, volume_id: QString, title: QString) {
        if self.with_app_mut(|app| {
            if let Err(e) = app.create_new_chapter(project_id, volume_id, title) {
                app.set_error(&format!("创建章节失败: {}", e));
            }
        }).is_ok() {
            self.emit_changed();
        }
    }
    fn rename_project(&mut self, project_id: QString, new_title: QString) -> QJsonObject {
        let result = self.with_app_mut(|app| {
            app.rename_project_json(project_id, new_title)
        });
        if result.is_ok() {
            self.emit_changed();
        }
        let out = result.unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into());
        qjson_object_from_json(&out.to_string())
    }
    fn delete_project(&mut self, project_id: QString) {
        if self.with_app_mut(|app| {
            if let Err(e) = app.delete_project(project_id) {
                app.set_error(&format!("删除作品失败: {}", e));
            }
        }).is_ok() {
            self.emit_changed();
        }
    }
    fn reorder_projects(&mut self, ordered_ids_joined: QString) {
        if self.with_app_mut(|app| app.reorder_projects(ordered_ids_joined)).is_ok() {
            self.emit_changed();
        }
    }
    fn rename_volume(
        &mut self,
        project_id: QString,
        volume_id: QString,
        new_title: QString,
    ) -> QJsonObject {
        let result = self.with_app_mut(|app| {
            app.rename_volume_json(project_id, volume_id, new_title)
        });
        if result.is_ok() {
            self.emit_changed();
        }
        let out = result.unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into());
        qjson_object_from_json(&out.to_string())
    }
    fn delete_volume(&mut self, project_id: QString, volume_id: QString) {
        if self.with_app_mut(|app| {
            if let Err(e) = app.delete_volume(project_id, volume_id) {
                app.set_error(&format!("删除分卷失败: {}", e));
            }
        }).is_ok() {
            self.emit_changed();
        }
    }
    fn reorder_volumes(&mut self, project_id: QString, ordered_ids_joined: QString) {
        if self.with_app_mut(|app| {
            app.reorder_volumes(project_id, ordered_ids_joined)
        }).is_ok() {
            self.emit_changed();
        }
    }
    fn rename_chapter(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        new_title: QString,
    ) -> QJsonObject {
        let result = self.with_app_mut(|app| {
            app.rename_chapter_json(project_id, volume_id, chapter_id, new_title)
        });
        if result.is_ok() {
            self.emit_changed();
        }
        let out = result.unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into());
        qjson_object_from_json(&out.to_string())
    }
    fn delete_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) {
        if self.with_app_mut(|app| {
            if let Err(e) = app.delete_chapter(project_id, volume_id, chapter_id) {
                app.set_error(&format!("删除章节失败: {}", e));
            }
        }).is_ok() {
            self.emit_changed();
        }
    }
    fn reorder_chapters(
        &mut self,
        project_id: QString,
        volume_id: QString,
        ordered_ids_joined: QString,
    ) {
        if self.with_app_mut(|app| {
            app.reorder_chapters(project_id, volume_id, ordered_ids_joined)
        }).is_ok() {
            self.emit_changed();
        }
    }
    fn select_project(&mut self, project_id: QString) {
        if self.with_app_mut(|app| app.select_project(project_id)).is_ok() {
            self.selected_item_changed();
        }
    }
    fn select_volume(&mut self, project_id: QString, volume_id: QString) {
        if self.with_app_mut(|app| app.select_volume(project_id, volume_id)).is_ok() {
            self.selected_item_changed();
        }
    }
    fn select_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) {
        if self.with_app_mut(|app| {
            app.select_chapter(project_id, volume_id, chapter_id)
        }).is_ok() {
            self.selected_item_changed();
        }
    }
}
