use qmetaobject::prelude::*;
use qmetaobject::{QJsonArray, QJsonObject, QJsonValue};
use std::ffi::CStr;
use std::rc::Rc;
use std::cell::RefCell;
use rfd::FileDialog;

use writer_core::facade::WriterCore;

qmetaobject::qrc!(qml_resources, "qml" { "qml/main.qml" });

#[derive(QObject, Default)]
struct AppBackend {
    base: qt_base_class!(trait QObject),

    workspacePath: qt_property!(QString; READ workspace_path NOTIFY workspaceOpened),
    saveStatus: qt_property!(QString; READ save_status WRITE set_save_status NOTIFY saveStatusChanged),
    wordCount: qt_property!(i32; READ word_count WRITE set_word_count NOTIFY wordCountChanged),

    workspaceOpened: qt_signal!(),
    projectsReloaded: qt_signal!(),
    saveStatusChanged: qt_signal!(),
    wordCountChanged: qt_signal!(),

    openWorkspaceDialog: qt_method!(fn(&mut self)),
    createNewProject: qt_method!(fn(&mut self)),
    createNewVolume: qt_method!(fn(&mut self, project_id: QString)),
    createNewChapter: qt_method!(fn(&mut self, project_id: QString, volume_id: QString)),

    getTreeModel: qt_method!(fn(&self) -> QJsonArray),

    selectProject: qt_method!(fn(&mut self, project_id: QString)),
    selectVolume: qt_method!(fn(&mut self, project_id: QString, volume_id: QString)),
    selectChapter: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString)),

    getChapterContent: qt_method!(fn(&self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString),
    saveCurrentChapter: qt_method!(fn(&mut self, content: QString)),

    core: Option<Rc<RefCell<WriterCore>>>,
    current_workspace: String,
    current_save_status: String,
    current_word_count: i32,

    selected_project_id: Option<String>,
    selected_volume_id: Option<String>,
    selected_chapter_id: Option<String>,

    cached_tree: QJsonArray,
}

impl AppBackend {
    fn workspace_path(&self) -> QString {
        self.current_workspace.clone().into()
    }

    fn save_status(&self) -> QString {
        self.current_save_status.clone().into()
    }

    fn set_save_status(&mut self, status: QString) {
        self.current_save_status = status.to_string();
        self.saveStatusChanged();
    }

    fn word_count(&self) -> i32 {
        self.current_word_count
    }

    fn set_word_count(&mut self, count: i32) {
        self.current_word_count = count;
        self.wordCountChanged();
    }

    fn openWorkspaceDialog(&mut self) {
        if let Some(path) = FileDialog::new().pick_folder() {
            let core = WriterCore::new(&path);

            // Validate first, if invalid try to create.
            if !core.validate_workspace().unwrap_or(false) {
                if let Err(e) = core.create_workspace() {
                    println!("Error creating workspace: {}", e);
                    return;
                }
                if !core.validate_workspace().unwrap_or(false) {
                    println!("Invalid workspace even after creation");
                    return;
                }
            }

            self.core = Some(Rc::new(RefCell::new(core)));
            self.current_workspace = path.to_string_lossy().to_string();
            self.current_save_status = "已保存".to_string();
            self.saveStatusChanged();
            self.reload_tree();
            self.workspaceOpened();
        }
    }

    fn reload_tree(&mut self) {
        let mut list = QJsonArray::default();
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
            if let Ok(projects) = core.list_projects() {
                for p in projects {
                    let mut p_map = QJsonObject::default();
                    p_map.insert("title".into(), QJsonValue::from(QString::from(p.title.clone())));
                    p_map.insert("id".into(), QJsonValue::from(QString::from(p.id.clone())));
                    p_map.insert("type".into(), QJsonValue::from(QString::from("project")));
                    list.push(QJsonValue::from(p_map));

                    if let Ok(volumes) = core.list_volumes(&p.id) {
                        for v in volumes {
                            let mut v_map = QJsonObject::default();
                            v_map.insert("title".into(), QJsonValue::from(QString::from(format!("  {}", v.title))));
                            v_map.insert("id".into(), QJsonValue::from(QString::from(v.id.clone())));
                            v_map.insert("projectId".into(), QJsonValue::from(QString::from(p.id.clone())));
                            v_map.insert("type".into(), QJsonValue::from(QString::from("volume")));
                            list.push(QJsonValue::from(v_map));

                            if let Ok(chapters) = core.list_chapters(&p.id, &v.id) {
                                for c in chapters {
                                    let mut c_map = QJsonObject::default();
                                    c_map.insert("title".into(), QJsonValue::from(QString::from(format!("    {}", c.title))));
                                    c_map.insert("id".into(), QJsonValue::from(QString::from(c.id.clone())));
                                    c_map.insert("projectId".into(), QJsonValue::from(QString::from(p.id.clone())));
                                    c_map.insert("volumeId".into(), QJsonValue::from(QString::from(v.id.clone())));
                                    c_map.insert("type".into(), QJsonValue::from(QString::from("chapter")));
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

    fn getTreeModel(&self) -> QJsonArray {
        self.cached_tree.clone()
    }

    fn createNewProject(&mut self) {
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
            if core.create_project("新作品").is_ok() {
                drop(core);
                self.reload_tree();
                self.projectsReloaded();
            }
        }
    }

    fn createNewVolume(&mut self, project_id: QString) {
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
            if core.create_volume(&project_id.to_string(), "新分卷").is_ok() {
                drop(core);
                self.reload_tree();
                self.projectsReloaded();
            }
        }
    }

    fn createNewChapter(&mut self, project_id: QString, volume_id: QString) {
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
            if core.create_chapter(&project_id.to_string(), &volume_id.to_string(), "新章节").is_ok() {
                drop(core);
                self.reload_tree();
                self.projectsReloaded();
            }
        }
    }

    fn selectProject(&mut self, project_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
    }

    fn selectVolume(&mut self, project_id: QString, volume_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = Some(volume_id.to_string());
    }

    fn selectChapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = Some(volume_id.to_string());
        self.selected_chapter_id = Some(chapter_id.to_string());
    }

    fn getChapterContent(&self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString {
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
            if let Ok(content) = core.read_chapter(&project_id.to_string(), &volume_id.to_string(), &chapter_id.to_string()) {
                return content.content.into();
            }
        }
        "".into()
    }

    fn saveCurrentChapter(&mut self, content: QString) {
        if let (Some(core_ref), Some(p), Some(v), Some(c)) = (&self.core, &self.selected_project_id, &self.selected_volume_id, &self.selected_chapter_id) {
            let core = core_ref.borrow();
            if core.write_chapter(p, v, c, &content.to_string()).is_ok() {
                self.current_save_status = "已保存".to_string();
                self.saveStatusChanged();
            }
        }
    }
}

fn main() {
    qml_resources();
    qmetaobject::qml_register_type::<AppBackend>(CStr::from_bytes_with_nul(b"WriterApp\0").unwrap(), 1, 0, CStr::from_bytes_with_nul(b"AppBackend\0").unwrap());

    let mut engine = QmlEngine::new();
    engine.load_file("qrc:/qml/qml/main.qml".into());
    engine.exec();
}
