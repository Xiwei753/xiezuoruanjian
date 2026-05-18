use qmetaobject::prelude::*;
use qmetaobject::{QJsonArray, QJsonObject, QJsonValue};
use rfd::FileDialog;
use std::cell::RefCell;
use std::ffi::CStr;
use std::rc::Rc;

use writer_core::facade::WriterCore;

qmetaobject::qrc!(qml_resources, "/" { "qml/main.qml" as "main.qml" });

#[allow(dead_code)]
#[derive(QObject, Default)]
struct AppBackend {
    base: qt_base_class!(trait QObject),

    workspace_path: qt_property!(QString; READ workspace_path NOTIFY workspace_opened),
    save_status: qt_property!(QString; READ save_status WRITE set_save_status NOTIFY save_status_changed),
    word_count: qt_property!(i32; READ word_count WRITE set_word_count NOTIFY word_count_changed),
    error_message: qt_property!(QString; READ error_message NOTIFY error_occurred),
    selected_item_id: qt_property!(QString; READ selected_item_id NOTIFY selected_item_changed),
    has_selected_chapter_prop: qt_property!(bool; READ has_selected_chapter_prop NOTIFY selected_item_changed),
    chapter_path: qt_property!(QString; READ chapter_path NOTIFY chapter_path_changed),

    workspace_opened: qt_signal!(),
    projects_reloaded: qt_signal!(),
    save_status_changed: qt_signal!(),
    word_count_changed: qt_signal!(),
    error_occurred: qt_signal!(),
    selected_item_changed: qt_signal!(),
    chapter_path_changed: qt_signal!(),
    clear_editor: qt_signal!(),

    sync_enabled: qt_property!(bool; READ sync_enabled WRITE set_sync_enabled NOTIFY sync_config_changed),
    sync_remote_url: qt_property!(QString; READ sync_remote_url WRITE set_sync_remote_url NOTIFY sync_config_changed),
    sync_branch: qt_property!(QString; READ sync_branch WRITE set_sync_branch NOTIFY sync_config_changed),
    sync_auto_sync: qt_property!(bool; READ sync_auto_sync WRITE set_sync_auto_sync NOTIFY sync_config_changed),
    sync_interval: qt_property!(u32; READ sync_interval WRITE set_sync_interval NOTIFY sync_config_changed),
    sync_proxy_type: qt_property!(QString; READ sync_proxy_type WRITE set_sync_proxy_type NOTIFY sync_config_changed),
    sync_proxy_host: qt_property!(QString; READ sync_proxy_host WRITE set_sync_proxy_host NOTIFY sync_config_changed),
    sync_proxy_port: qt_property!(u16; READ sync_proxy_port WRITE set_sync_proxy_port NOTIFY sync_config_changed),
    sync_token: qt_property!(QString; READ sync_token WRITE set_sync_token NOTIFY sync_config_changed),

    sync_config_changed: qt_signal!(),
    sync_action_result: qt_property!(QString; READ sync_action_result NOTIFY sync_action_completed),
    sync_action_completed: qt_signal!(),

    load_sync_config: qt_method!(fn(&mut self)),
    save_sync_config: qt_method!(fn(&mut self) -> bool),
    perform_sync_dry_run: qt_method!(fn(&mut self)),
    perform_sync: qt_method!(fn(&mut self)),

    open_workspace_dialog: qt_method!(fn(&mut self)),
    create_new_project: qt_method!(fn(&mut self, title: QString)),
    create_new_volume: qt_method!(fn(&mut self, project_id: QString, title: QString)),
    create_new_chapter:
        qt_method!(fn(&mut self, project_id: QString, volume_id: QString, title: QString)),

    rename_project: qt_method!(fn(&mut self, project_id: QString, new_title: QString)),
    delete_project: qt_method!(fn(&mut self, project_id: QString)),
    reorder_projects: qt_method!(fn(&mut self, ordered_ids_joined: QString)),

    rename_volume:
        qt_method!(fn(&mut self, project_id: QString, volume_id: QString, new_title: QString)),
    delete_volume: qt_method!(fn(&mut self, project_id: QString, volume_id: QString)),
    reorder_volumes: qt_method!(fn(&mut self, project_id: QString, ordered_ids_joined: QString)),

    rename_chapter: qt_method!(
        fn(
            &mut self,
            project_id: QString,
            volume_id: QString,
            chapter_id: QString,
            new_title: QString,
        )
    ),
    delete_chapter:
        qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString)),
    reorder_chapters: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, ordered_ids_joined: QString)
    ),

    get_tree_model: qt_method!(fn(&self) -> QJsonArray),
    calculate_word_count: qt_method!(fn(&mut self, text: QString)),

    select_project: qt_method!(fn(&mut self, project_id: QString)),
    select_volume: qt_method!(fn(&mut self, project_id: QString, volume_id: QString)),
    select_chapter:
        qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString)),

    get_chapter_content: qt_method!(
        fn(&self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString
    ),
    save_current_chapter: qt_method!(fn(&mut self, content: QString)),

    has_selected_chapter: qt_method!(fn(&self) -> bool),
    selected_chapter_exists: qt_method!(fn(&self) -> bool),
    clear_editor_state: qt_method!(fn(&mut self)),

    core: Option<Rc<RefCell<WriterCore>>>,
    current_workspace: String,
    current_save_status: String,
    current_word_count: i32,
    current_error_message: String,

    selected_project_id: Option<String>,
    selected_volume_id: Option<String>,
    selected_chapter_id: Option<String>,

    cached_tree: QJsonArray,

    current_sync_enabled: bool,
    current_sync_remote_url: String,
    current_sync_branch: String,
    current_sync_auto_sync: bool,
    current_sync_interval: u32,
    current_sync_proxy_type: String,
    current_sync_proxy_host: String,
    current_sync_proxy_port: u16,
    current_sync_token: String,
    current_sync_action_result: String,
}

impl AppBackend {
    fn sync_enabled(&self) -> bool {
        self.current_sync_enabled
    }
    fn set_sync_enabled(&mut self, val: bool) {
        self.current_sync_enabled = val;
        self.sync_config_changed();
    }

    fn sync_remote_url(&self) -> QString {
        self.current_sync_remote_url.clone().into()
    }
    fn set_sync_remote_url(&mut self, val: QString) {
        self.current_sync_remote_url = val.to_string();
        self.sync_config_changed();
    }

    fn sync_branch(&self) -> QString {
        self.current_sync_branch.clone().into()
    }
    fn set_sync_branch(&mut self, val: QString) {
        self.current_sync_branch = val.to_string();
        self.sync_config_changed();
    }

    fn sync_auto_sync(&self) -> bool {
        self.current_sync_auto_sync
    }
    fn set_sync_auto_sync(&mut self, val: bool) {
        self.current_sync_auto_sync = val;
        self.sync_config_changed();
    }

    fn sync_interval(&self) -> u32 {
        self.current_sync_interval
    }
    fn set_sync_interval(&mut self, val: u32) {
        self.current_sync_interval = val;
        self.sync_config_changed();
    }

    fn sync_proxy_type(&self) -> QString {
        self.current_sync_proxy_type.clone().into()
    }
    fn set_sync_proxy_type(&mut self, val: QString) {
        self.current_sync_proxy_type = val.to_string();
        self.sync_config_changed();
    }

    fn sync_proxy_host(&self) -> QString {
        self.current_sync_proxy_host.clone().into()
    }
    fn set_sync_proxy_host(&mut self, val: QString) {
        self.current_sync_proxy_host = val.to_string();
        self.sync_config_changed();
    }

    fn sync_proxy_port(&self) -> u16 {
        self.current_sync_proxy_port
    }
    fn set_sync_proxy_port(&mut self, val: u16) {
        self.current_sync_proxy_port = val;
        self.sync_config_changed();
    }

    fn sync_token(&self) -> QString {
        self.current_sync_token.clone().into()
    }
    fn set_sync_token(&mut self, val: QString) {
        self.current_sync_token = val.to_string();
        self.sync_config_changed();
    }

    fn sync_action_result(&self) -> QString {
        self.current_sync_action_result.clone().into()
    }

    fn load_sync_config(&mut self) {
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
            if let Ok(config) = core.load_sync_config() {
                self.current_sync_enabled = config.enabled;
                self.current_sync_remote_url = config.remote_url.clone();
                self.current_sync_branch = config.branch.clone();
                self.current_sync_auto_sync = config.auto_sync;
                self.current_sync_interval = config.sync_interval_seconds;
                self.current_sync_proxy_type = config.proxy_type.clone();
                self.current_sync_proxy_host = config.proxy_host.clone();
                self.current_sync_proxy_port = config.proxy_port;
            }
            if let Ok(secrets) = core.load_sync_secrets() {
                if let Some(t) = secrets.token {
                    self.current_sync_token = t;
                } else {
                    self.current_sync_token = "".to_string();
                }
            }
            self.sync_config_changed();
        }
    }

    fn save_sync_config(&mut self) -> bool {
        let mut error_msg: Option<String> = None;
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();

            let mut c = core
                .load_sync_config()
                .unwrap_or(writer_core::sync_service::SyncConfig {
                    enabled: false,
                    remote_url: "".to_string(),
                    transport: writer_core::sync_service::SyncTransport::HttpsToken,
                    branch: "main".to_string(),
                    auto_sync: false,
                    sync_interval_seconds: 300,
                    proxy_enabled: false,
                    proxy_type: "none".to_string(),
                    proxy_host: "".to_string(),
                    proxy_port: 0,
                });

            c.enabled = self.current_sync_enabled;
            c.remote_url = self.current_sync_remote_url.clone();
            c.branch = self.current_sync_branch.clone();
            c.auto_sync = self.current_sync_auto_sync;
            c.sync_interval_seconds = self.current_sync_interval;
            c.proxy_type = self.current_sync_proxy_type.clone();
            if c.proxy_type != "none" {
                c.proxy_enabled = true;
            } else {
                c.proxy_enabled = false;
            }
            c.proxy_host = self.current_sync_proxy_host.clone();
            c.proxy_port = self.current_sync_proxy_port;

            let mut s = core.load_sync_secrets().unwrap_or_default();
            s.token = if self.current_sync_token.is_empty() {
                None
            } else {
                Some(self.current_sync_token.clone())
            };

            if let Err(e) = core.save_sync_config(&c) {
                error_msg = Some(format!("保存同步配置失败: {}", e));
            } else if let Err(e) = core.save_sync_secrets(&s) {
                error_msg = Some(format!("保存同步凭证失败: {}", e));
            }
        }

        if let Some(msg) = error_msg {
            self.set_error(&msg);
            self.current_sync_action_result = msg;
            self.sync_action_completed();
            return false;
        }

        self.current_sync_action_result = "配置保存成功".to_string();
        self.sync_action_completed();
        true
    }

    fn perform_sync_dry_run(&mut self) {
        let mut error_msg: Option<String> = None;
        let mut lines = Vec::new();

        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
            let config = match core.load_sync_config() {
                Ok(c) => c,
                Err(e) => {
                    error_msg = Some(format!("无法读取同步配置: {}", e));
                    // we need to break out early, but we are inside `if let`.
                    writer_core::sync_service::SyncConfig {
                        enabled: false,
                        remote_url: "".to_string(),
                        transport: writer_core::sync_service::SyncTransport::HttpsToken,
                        branch: "main".to_string(),
                        auto_sync: false,
                        sync_interval_seconds: 300,
                        proxy_enabled: false,
                        proxy_type: "none".to_string(),
                        proxy_host: "".to_string(),
                        proxy_port: 0,
                    }
                }
            };

            if error_msg.is_none() {
                match core.perform_sync_dry_run(&config) {
                    Ok(plan) => {
                        if !plan.files_to_upload.is_empty() {
                            lines.push(format!("准备上传: {} 个文件", plan.files_to_upload.len()));
                            for f in plan.files_to_upload.iter().take(5) {
                                lines.push(format!("  + {}", f));
                            }
                            if plan.files_to_upload.len() > 5 {
                                lines.push("  ...".to_string());
                            }
                        }
                        if !plan.files_to_download.is_empty() {
                            lines
                                .push(format!("准备下载: {} 个文件", plan.files_to_download.len()));
                            for f in plan.files_to_download.iter().take(5) {
                                lines.push(format!("  ↓ {}", f));
                            }
                        }
                        if !plan.files_to_delete_local.is_empty() {
                            lines.push(format!(
                                "准备本地删除: {} 个文件",
                                plan.files_to_delete_local.len()
                            ));
                        }
                        if !plan.files_to_delete_remote.is_empty() {
                            lines.push(format!(
                                "准备远端删除: {} 个文件",
                                plan.files_to_delete_remote.len()
                            ));
                        }
                        if !plan.ignored_files.is_empty() {
                            lines.push(format!("忽略文件: {} 个", plan.ignored_files.len()));
                        }

                        if lines.is_empty() {
                            lines.push("没有需要同步的变更。".to_string());
                        }
                    }
                    Err(e) => {
                        error_msg = Some(format!("检查计划失败:\n{}", e));
                    }
                }
            }
        }

        if let Some(msg) = error_msg {
            self.set_error(&msg);
            self.current_sync_action_result = msg;
        } else {
            self.current_sync_action_result = lines.join("\n");
        }
        self.sync_action_completed();
    }

    fn perform_sync(&mut self) {
        let mut error_msg: Option<String> = None;
        let mut result_msg: Option<String> = None;

        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
            let config = match core.load_sync_config() {
                Ok(c) => c,
                Err(e) => {
                    error_msg = Some(format!("无法读取同步配置: {}", e));
                    writer_core::sync_service::SyncConfig {
                        enabled: false,
                        remote_url: "".to_string(),
                        transport: writer_core::sync_service::SyncTransport::HttpsToken,
                        branch: "main".to_string(),
                        auto_sync: false,
                        sync_interval_seconds: 300,
                        proxy_enabled: false,
                        proxy_type: "none".to_string(),
                        proxy_host: "".to_string(),
                        proxy_port: 0,
                    }
                }
            };

            if error_msg.is_none() {
                match core.perform_sync(&config) {
                    Ok(result) => {
                        let status_str = match result.status {
                            writer_core::sync_service::SyncStatus::Success => "同步成功",
                            writer_core::sync_service::SyncStatus::Error(ref e) => {
                                error_msg = Some(format!("同步失败:\n{}", e));
                                ""
                            }
                            writer_core::sync_service::SyncStatus::Conflict => "同步冲突",
                            _ => "同步未知状态",
                        };

                        if error_msg.is_none() {
                            result_msg = Some(format!(
                                "{}\n上传: {} 个文件\n下载: {} 个文件",
                                status_str,
                                result.uploaded_files.len(),
                                result.downloaded_files.len()
                            ));
                        }
                    }
                    Err(e) => {
                        error_msg = Some(format!("同步操作失败:\n{}", e));
                    }
                }
            }
        }

        if let Some(msg) = error_msg {
            self.current_sync_action_result = msg.clone();
            self.set_error(&msg);
        } else if let Some(msg) = result_msg {
            self.current_sync_action_result = msg;
            self.reload_tree();
            self.projects_reloaded();
        }

        self.sync_action_completed();
    }

    fn workspace_path(&self) -> QString {
        self.current_workspace.clone().into()
    }

    fn save_status(&self) -> QString {
        self.current_save_status.clone().into()
    }

    fn set_save_status(&mut self, status: QString) {
        self.current_save_status = status.to_string();
        self.save_status_changed();
    }

    fn word_count(&self) -> i32 {
        self.current_word_count
    }

    fn set_word_count(&mut self, count: i32) {
        self.current_word_count = count;
        self.word_count_changed();
    }

    fn error_message(&self) -> QString {
        self.current_error_message.clone().into()
    }

    fn has_selected_chapter(&self) -> bool {
        self.selected_chapter_id.is_some()
    }

    fn selected_chapter_exists(&self) -> bool {
        if let (Some(core_ref), Some(p), Some(v), Some(c)) = (
            &self.core,
            &self.selected_project_id,
            &self.selected_volume_id,
            &self.selected_chapter_id,
        ) {
            let core = core_ref.borrow();
            if let Ok(chapters) = core.list_chapters(p, v) {
                return chapters.iter().any(|chap| chap.id == *c);
            }
        }
        false
    }

    fn clear_editor_state(&mut self) {
        self.selected_chapter_id = None;
        self.selected_item_changed();
        self.chapter_path_changed();
        self.clear_editor();
    }

    fn chapter_path(&self) -> QString {
        if let (Some(core_ref), Some(p), Some(v), Some(c)) = (
            &self.core,
            &self.selected_project_id,
            &self.selected_volume_id,
            &self.selected_chapter_id,
        ) {
            let core = core_ref.borrow();
            // Just format it nicely
            let mut path = String::new();
            if let Ok(projects) = core.list_projects() {
                if let Some(proj) = projects.iter().find(|x| x.id == *p) {
                    path.push_str(&proj.title);
                }
            }
            if let Ok(volumes) = core.list_volumes(p) {
                if let Some(vol) = volumes.iter().find(|x| x.id == *v) {
                    path.push_str(" > ");
                    path.push_str(&vol.title);
                }
            }
            if let Ok(chapters) = core.list_chapters(p, v) {
                if let Some(chap) = chapters.iter().find(|x| x.id == *c) {
                    path.push_str(" > ");
                    path.push_str(&chap.title);
                }
            }
            return path.into();
        }
        "".into()
    }

    fn has_selected_chapter_prop(&self) -> bool {
        self.selected_chapter_id.is_some()
    }

    fn selected_item_id(&self) -> QString {
        if let Some(ref id) = self.selected_chapter_id {
            return id.clone().into();
        }
        if let Some(ref id) = self.selected_volume_id {
            return id.clone().into();
        }
        if let Some(ref id) = self.selected_project_id {
            return id.clone().into();
        }
        "".into()
    }

    fn set_error(&mut self, msg: &str) {
        self.current_error_message = msg.to_string();
        self.error_occurred();
    }

    fn calculate_word_count(&mut self, text: QString) {
        let text_str = text.to_string();
        let count = text_str.chars().filter(|c| !c.is_whitespace()).count() as i32;
        self.set_word_count(count);
    }

    fn open_workspace_dialog(&mut self) {
        if let Some(path) = FileDialog::new().pick_folder() {
            let core = WriterCore::new(&path);

            // Validate first, if invalid try to create.
            if !core.validate_workspace().unwrap_or(false) {
                if let Err(e) = core.create_workspace() {
                    self.set_error(&format!("无法创建工作区: {}", e));
                    return;
                }
                if !core.validate_workspace().unwrap_or(false) {
                    self.set_error("创建后工作区依然无效");
                    return;
                }
            }

            self.core = Some(Rc::new(RefCell::new(core)));
            self.current_workspace = path.to_string_lossy().to_string();
            self.current_save_status = "已保存".to_string();
            self.save_status_changed();
            self.reload_tree();
            self.workspace_opened();
        }
    }

    fn reload_tree(&mut self) {
        let mut list = QJsonArray::default();
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
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
    }

    fn get_tree_model(&self) -> QJsonArray {
        self.cached_tree.clone()
    }

    fn create_new_project(&mut self, title: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.create_project(&title.to_string())
            };
            match result {
                Ok(proj) => {
                    self.selected_project_id = Some(proj.id.clone());
                    self.selected_item_changed();
                    self.selected_volume_id = None;
                    self.selected_chapter_id = None;
                    self.reload_tree();
                    self.projects_reloaded();
                }
                Err(e) => self.set_error(&format!("创建作品失败: {}", e)),
            }
        }
    }

    fn create_new_volume(&mut self, project_id: QString, title: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.create_volume(&project_id.to_string(), &title.to_string())
            };
            match result {
                Ok(vol) => {
                    self.selected_project_id = Some(project_id.to_string());
                    self.selected_volume_id = Some(vol.id.clone());
                    self.selected_item_changed();
                    self.selected_chapter_id = None;
                    self.reload_tree();
                    self.projects_reloaded();
                }
                Err(e) => self.set_error(&format!("创建分卷失败: {}", e)),
            }
        }
    }

    fn create_new_chapter(&mut self, project_id: QString, volume_id: QString, title: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.create_chapter(
                    &project_id.to_string(),
                    &volume_id.to_string(),
                    &title.to_string(),
                )
            };
            match result {
                Ok(chap) => {
                    self.selected_project_id = Some(project_id.to_string());
                    self.selected_volume_id = Some(volume_id.to_string());
                    self.selected_chapter_id = Some(chap.id.clone());
                    self.selected_item_changed();
                    self.reload_tree();
                    self.projects_reloaded();
                }
                Err(e) => self.set_error(&format!("创建章节失败: {}", e)),
            }
        }
    }

    fn rename_project(&mut self, project_id: QString, new_title: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.rename_project(&project_id.to_string(), &new_title.to_string())
            };
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重命名作品失败: {}", e)),
            }
        }
    }

    fn delete_project(&mut self, project_id: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.delete_project(&project_id.to_string())
            };
            match result {
                Ok(_) => {
                    if self.selected_project_id.as_deref() == Some(&project_id.to_string()) {
                        self.selected_project_id = None;
                        self.selected_volume_id = None;
                        self.clear_editor_state();
                    }

                    self.reload_tree();
                    self.projects_reloaded();
                }
                Err(e) => self.set_error(&format!("删除作品失败: {}", e)),
            }
        }
    }

    fn reorder_projects(&mut self, ordered_ids_joined: QString) {
        if let Some(core_ref) = &self.core {
            let ordered_ids_str = ordered_ids_joined.to_string();
            let ids: Vec<String> = ordered_ids_str
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            let result = {
                let core = core_ref.borrow();
                core.reorder_projects(&ids)
            };
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重排作品失败: {}", e)),
            }
        }
    }

    fn rename_volume(&mut self, project_id: QString, volume_id: QString, new_title: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.rename_volume(
                    &project_id.to_string(),
                    &volume_id.to_string(),
                    &new_title.to_string(),
                )
            };
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重命名分卷失败: {}", e)),
            }
        }
    }

    fn delete_volume(&mut self, project_id: QString, volume_id: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.delete_volume(&project_id.to_string(), &volume_id.to_string())
            };
            match result {
                Ok(_) => {
                    if self.selected_volume_id.as_deref() == Some(&volume_id.to_string()) {
                        self.selected_volume_id = None;
                        self.clear_editor_state();
                    }

                    self.reload_tree();
                    self.projects_reloaded();
                }
                Err(e) => self.set_error(&format!("删除分卷失败: {}", e)),
            }
        }
    }

    fn reorder_volumes(&mut self, project_id: QString, ordered_ids_joined: QString) {
        if let Some(core_ref) = &self.core {
            let ordered_ids_str = ordered_ids_joined.to_string();
            let ids: Vec<String> = ordered_ids_str
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            let result = {
                let core = core_ref.borrow();
                core.reorder_volumes(&project_id.to_string(), &ids)
            };
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重排分卷失败: {}", e)),
            }
        }
    }

    fn rename_chapter(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        new_title: QString,
    ) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.rename_chapter(
                    &project_id.to_string(),
                    &volume_id.to_string(),
                    &chapter_id.to_string(),
                    &new_title.to_string(),
                )
            };
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重命名章节失败: {}", e)),
            }
        }
    }

    fn delete_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.delete_chapter(
                    &project_id.to_string(),
                    &volume_id.to_string(),
                    &chapter_id.to_string(),
                )
            };
            match result {
                Ok(_) => {
                    if self.selected_chapter_id.as_deref() == Some(&chapter_id.to_string()) {
                        self.clear_editor_state();
                    }

                    self.reload_tree();
                    self.projects_reloaded();
                }
                Err(e) => self.set_error(&format!("删除章节失败: {}", e)),
            }
        }
    }

    fn reorder_chapters(
        &mut self,
        project_id: QString,
        volume_id: QString,
        ordered_ids_joined: QString,
    ) {
        if let Some(core_ref) = &self.core {
            let ordered_ids_str = ordered_ids_joined.to_string();
            let ids: Vec<String> = ordered_ids_str
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            let result = {
                let core = core_ref.borrow();
                core.reorder_chapters(&project_id.to_string(), &volume_id.to_string(), &ids)
            };
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重排章节失败: {}", e)),
            }
        }
    }

    fn select_project(&mut self, project_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_item_changed();
        self.chapter_path_changed();
    }

    fn select_volume(&mut self, project_id: QString, volume_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = Some(volume_id.to_string());
        self.selected_item_changed();
        self.chapter_path_changed();
    }

    fn select_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = Some(volume_id.to_string());
        self.selected_chapter_id = Some(chapter_id.to_string());
        self.selected_item_changed();
        self.chapter_path_changed();
    }

    fn get_chapter_content(
        &self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> QString {
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
            if let Ok(content) = core.read_chapter(
                &project_id.to_string(),
                &volume_id.to_string(),
                &chapter_id.to_string(),
            ) {
                return content.content.into();
            }
        }
        "".into()
    }

    fn save_current_chapter(&mut self, content: QString) {
        if !self.selected_chapter_exists() {
            self.clear_editor_state();
            self.set_error("当前章节已不存在，已停止保存。");
            return;
        }

        let save_result = if let (Some(core_ref), Some(p), Some(v), Some(c)) = (
            &self.core,
            &self.selected_project_id,
            &self.selected_volume_id,
            &self.selected_chapter_id,
        ) {
            let core = core_ref.borrow();
            core.write_chapter(p, v, c, &content.to_string())
        } else {
            return;
        };

        match save_result {
            Ok(_) => {
                self.current_save_status = "已保存".to_string();
                self.save_status_changed();
            }
            Err(e) => {
                self.set_error(&format!("保存失败: {}", e));
            }
        }
    }
}

fn main() {
    qml_resources();
    qmetaobject::qml_register_type::<AppBackend>(
        CStr::from_bytes_with_nul(b"WriterApp\0").unwrap(),
        1,
        0,
        CStr::from_bytes_with_nul(b"AppBackend\0").unwrap(),
    );

    let mut engine = QmlEngine::new();
    engine.load_file("qrc:/main.qml".into());
    // In qmetaobject 0.2.10, QML load errors are automatically printed to stderr by the underlying Qt engine.
    engine.exec();
}
