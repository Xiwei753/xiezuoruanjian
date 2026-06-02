// =============================================================================
// project_backend.rs — 作品、分卷及章节生命周期领域 QObject 后端适配层
// =============================================================================

use super::*;
use crate::backend::SafeAppPtr;

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
    rename_project_json: qt_method!(fn(&mut self, project_id: QString, new_title: QString) -> QString),
    delete_project: qt_method!(fn(&mut self, project_id: QString)),
    reorder_projects: qt_method!(fn(&mut self, ordered_ids_joined: QString)),
    rename_volume_json: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, new_title: QString) -> QString),
    delete_volume: qt_method!(fn(&mut self, project_id: QString, volume_id: QString)),
    reorder_volumes: qt_method!(fn(&mut self, project_id: QString, ordered_ids_joined: QString)),
    rename_chapter_json: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, new_title: QString) -> QString),
    delete_chapter: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString)),
    reorder_chapters: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, ordered_ids_joined: QString)),
    select_project: qt_method!(fn(&mut self, project_id: QString)),
    select_volume: qt_method!(fn(&mut self, project_id: QString, volume_id: QString)),
    select_chapter: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString)),
    app: SafeAppPtr,
}

impl ProjectBackend {
    pub fn new(app: SafeAppPtr) -> Self { Self { app, ..Default::default() } }
    fn with_app<R>(&self, default: R, f: impl FnOnce(&AppBackend) -> R) -> R {
        if let Some(app) = self.app.get() {
            unsafe { f(&*app) }
        } else {
            crate::backend::app_backend::debug_error_static("project", "BACKEND_LINK_BROKEN", "app pointer is null");
            default
        }
    }
    fn with_app_mut<R>(&mut self, default: R, f: impl FnOnce(&mut AppBackend) -> R) -> R {
        if let Some(app) = self.app.get() {
            unsafe { f(&mut *app) }
        } else {
            crate::backend::app_backend::debug_error_static("project", "BACKEND_LINK_BROKEN", "app pointer is null");
            default
        }
    }
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
    fn create_new_volume(&mut self, project_id: QString, title: QString) { self.with_app_mut((), |app| { if let Err(e) = app.create_new_volume(project_id, title) { app.set_error(&format!("创建分卷失败: {}", e)); } }); self.emit_changed(); }
    fn create_new_chapter(&mut self, project_id: QString, volume_id: QString, title: QString) { self.with_app_mut((), |app| { if let Err(e) = app.create_new_chapter(project_id, volume_id, title) { app.set_error(&format!("创建章节失败: {}", e)); } }); self.emit_changed(); }
    fn rename_project_json(&mut self, project_id: QString, new_title: QString) -> QString { let out = self.with_app_mut("{}".into(), |app| app.rename_project_json(project_id, new_title)); self.emit_changed(); out }
    fn delete_project(&mut self, project_id: QString) { self.with_app_mut((), |app| { if let Err(e) = app.delete_project(project_id) { app.set_error(&format!("删除作品失败: {}", e)); } }); self.emit_changed(); }
    fn reorder_projects(&mut self, ordered_ids_joined: QString) { self.with_app_mut((), |app| app.reorder_projects(ordered_ids_joined)); self.emit_changed(); }
    fn rename_volume_json(&mut self, project_id: QString, volume_id: QString, new_title: QString) -> QString { let out = self.with_app_mut("{}".into(), |app| app.rename_volume_json(project_id, volume_id, new_title)); self.emit_changed(); out }
    fn delete_volume(&mut self, project_id: QString, volume_id: QString) { self.with_app_mut((), |app| { if let Err(e) = app.delete_volume(project_id, volume_id) { app.set_error(&format!("删除分卷失败: {}", e)); } }); self.emit_changed(); }
    fn reorder_volumes(&mut self, project_id: QString, ordered_ids_joined: QString) { self.with_app_mut((), |app| app.reorder_volumes(project_id, ordered_ids_joined)); self.emit_changed(); }
    fn rename_chapter_json(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, new_title: QString) -> QString { let out = self.with_app_mut("{}".into(), |app| app.rename_chapter_json(project_id, volume_id, chapter_id, new_title)); self.emit_changed(); out }
    fn delete_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) { self.with_app_mut((), |app| { if let Err(e) = app.delete_chapter(project_id, volume_id, chapter_id) { app.set_error(&format!("删除章节失败: {}", e)); } }); self.emit_changed(); }
    fn reorder_chapters(&mut self, project_id: QString, volume_id: QString, ordered_ids_joined: QString) { self.with_app_mut((), |app| app.reorder_chapters(project_id, volume_id, ordered_ids_joined)); self.emit_changed(); }
    fn select_project(&mut self, project_id: QString) { self.with_app_mut((), |app| app.select_project(project_id)); self.selected_item_changed(); }
    fn select_volume(&mut self, project_id: QString, volume_id: QString) { self.with_app_mut((), |app| app.select_volume(project_id, volume_id)); self.selected_item_changed(); }
    fn select_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) { self.with_app_mut((), |app| app.select_chapter(project_id, volume_id, chapter_id)); self.selected_item_changed(); }
}
