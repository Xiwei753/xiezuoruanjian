use eframe::egui;
use rfd::FileDialog;
use std::collections::HashMap;
use std::path::PathBuf;
use writer_core::chapter::Chapter;
use writer_core::facade::WriterCore;
use writer_core::project::Project;
use writer_core::volume::Volume;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1024.0, 768.0]),
        ..Default::default()
    };
    eframe::run_native(
        "写作软件 Linux MVP",
        options,
        Box::new(|_cc| Box::<WriterApp>::default()),
    )
}

struct AppState {
    workspace_path: Option<PathBuf>,
    core: Option<WriterCore>,

    // Loaded entities
    projects: Vec<Project>,
    cached_volumes: HashMap<String, Vec<Volume>>, // project_id -> volumes
    cached_chapters: HashMap<(String, String), Vec<Chapter>>, // (project_id, volume_id) -> chapters

    // UI state
    selected_project_id: Option<String>,
    selected_volume_id: Option<String>,
    selected_chapter_id: Option<String>,
    selected_chapter_title: Option<String>,

    // Creation states
    show_new_project_input: bool,
    new_project_name: String,
    show_new_volume_input: bool,
    new_volume_name: String,
    show_new_chapter_input: bool,
    new_chapter_name: String,

    // Management states
    rename_target: Option<(String, String, String)>, // (project_id, volume_id, chapter_id), empty string for parent
    rename_new_name: String,
    delete_target: Option<(String, String, String, String)>, // (project_id, volume_id, chapter_id, title)
    delete_target_type: Option<String>,                      // "project", "volume", "chapter"

    // Editor state
    chapter_content: String,

    // Auto-save state
    last_content: String,
    last_edit_time: Option<std::time::Instant>,

    // Error state
    error_message: Option<String>,
    save_message: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            workspace_path: None,
            core: None,
            projects: Vec::new(),
            cached_volumes: HashMap::new(),
            cached_chapters: HashMap::new(),
            selected_project_id: None,
            selected_volume_id: None,
            selected_chapter_id: None,
            selected_chapter_title: None,
            show_new_project_input: false,
            new_project_name: String::new(),
            show_new_volume_input: false,
            new_volume_name: String::new(),
            show_new_chapter_input: false,
            new_chapter_name: String::new(),
            rename_target: None,
            rename_new_name: String::new(),
            delete_target: None,
            delete_target_type: None,
            chapter_content: String::new(),
            last_content: String::new(),
            last_edit_time: None,
            error_message: None,
            save_message: None,
        }
    }
}

struct WriterApp {
    state: AppState,
}

impl Default for WriterApp {
    fn default() -> Self {
        Self {
            state: AppState::default(),
        }
    }
}

impl WriterApp {
    fn open_workspace(&mut self) {
        if let Some(path) = FileDialog::new().pick_folder() {
            let core = WriterCore::new(&path);
            match core.validate_workspace() {
                Ok(true) => {
                    self.state.workspace_path = Some(path);
                    self.state.core = Some(core);
                    self.state.error_message = None;
                    self.reload_projects();
                }
                Ok(false) | Err(_) => {
                    // Try to create workspace if invalid or not found
                    match core.create_workspace() {
                        Ok(_) => {
                            self.state.workspace_path = Some(path);
                            self.state.core = Some(core);
                            self.state.error_message = None;
                            self.reload_projects();
                        }
                        Err(e) => {
                            self.state.error_message = Some(format!("创建工作区失败: {}", e));
                        }
                    }
                }
            }
        }
    }

    fn reload_projects(&mut self) {
        if let Some(core) = &self.state.core {
            match core.list_projects() {
                Ok(projects) => {
                    self.state.projects = projects;
                    self.state.cached_volumes.clear();
                    self.state.cached_chapters.clear();
                }
                Err(e) => {
                    self.state.error_message = Some(format!("加载作品失败: {}", e));
                }
            }
        }
    }

    fn ensure_volumes_loaded(&mut self, project_id: &str) {
        if !self.state.cached_volumes.contains_key(project_id) {
            if let Some(core) = &self.state.core {
                match core.list_volumes(project_id) {
                    Ok(volumes) => {
                        self.state
                            .cached_volumes
                            .insert(project_id.to_string(), volumes);
                    }
                    Err(_) => {
                        self.state
                            .cached_volumes
                            .insert(project_id.to_string(), Vec::new());
                    }
                }
            }
        }
    }

    fn ensure_chapters_loaded(&mut self, project_id: &str, volume_id: &str) {
        let key = (project_id.to_string(), volume_id.to_string());
        if !self.state.cached_chapters.contains_key(&key) {
            if let Some(core) = &self.state.core {
                match core.list_chapters(project_id, volume_id) {
                    Ok(chapters) => {
                        self.state.cached_chapters.insert(key, chapters);
                    }
                    Err(_) => {
                        self.state.cached_chapters.insert(key, Vec::new());
                    }
                }
            }
        }
    }

    fn load_chapter(
        &mut self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        chapter_title: &str,
    ) {
        // Automatically save previous chapter if any
        self.save_chapter();
        // Clear last_edit_time so we don't accidentally save old content with new ID
        self.state.last_edit_time = None;

        self.state.selected_project_id = Some(project_id.to_string());
        self.state.selected_volume_id = Some(volume_id.to_string());
        self.state.selected_chapter_id = Some(chapter_id.to_string());
        self.state.selected_chapter_title = Some(chapter_title.to_string());
        self.state.save_message = None;
        self.state.error_message = None;

        if let Some(core) = &self.state.core {
            match core.read_chapter(project_id, volume_id, chapter_id) {
                Ok(content) => {
                    self.state.chapter_content = content.content.clone();
                    self.state.last_content = content.content;
                }
                Err(e) => {
                    self.state.error_message = Some(format!("读取章节失败: {}", e));
                    self.state.chapter_content = String::new();
                    self.state.last_content = String::new();
                }
            }
        }
    }

    fn save_chapter(&mut self) {
        if self.state.chapter_content == self.state.last_content {
            return; // No need to save if nothing changed
        }

        if let (Some(core), Some(p_id), Some(v_id), Some(c_id)) = (
            &self.state.core,
            &self.state.selected_project_id,
            &self.state.selected_volume_id,
            &self.state.selected_chapter_id,
        ) {
            match core.write_chapter(p_id, v_id, c_id, &self.state.chapter_content) {
                Ok(_) => {
                    self.state.save_message = Some("已保存".to_string());
                    self.state.last_content = self.state.chapter_content.clone();
                }
                Err(e) => {
                    self.state.error_message = Some(format!("保存章节失败: {}", e));
                }
            }
        }
    }
}

// Ensure saving happens when closing app
impl Drop for WriterApp {
    fn drop(&mut self) {
        self.save_chapter();
    }
}

// The Drop trait ensures self.save_chapter() is called when the app closes
// This covers the normal application exit flow.

impl eframe::App for WriterApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_chapter();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("打开/创建工作区").clicked() {
                    self.open_workspace();
                }
                if let Some(path) = &self.state.workspace_path {
                    ui.label(format!("工作区: {}", path.display()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(msg) = &self.state.save_message {
                        let color = if msg == "未保存" {
                            egui::Color32::YELLOW
                        } else if msg == "已保存" {
                            egui::Color32::GREEN
                        } else {
                            egui::Color32::RED
                        };
                        ui.label(egui::RichText::new(msg).color(color));
                    }
                    if self.state.selected_chapter_id.is_some() {
                        if ui.button("保存").clicked() {
                            self.save_chapter();
                        }
                    }
                });
            });
        });

        let mut clear_error = false;
        if let Some(err) = &self.state.error_message {
            egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::RED, err);
                    if ui.button("清除错误").clicked() {
                        clear_error = true;
                    }
                });
            });
        }
        if clear_error {
            self.state.error_message = None;
        }

        // Check if there is a pending delete confirmation
        let mut confirm_delete = false;
        let mut cancel_delete = false;

        if let (Some((_p_id, _v_id, c_id, title)), Some(t_type)) =
            (&self.state.delete_target, &self.state.delete_target_type)
        {
            egui::Window::new("确认删除")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "你确定要删除{} \"{}\" 吗？",
                        if t_type == "project" {
                            "作品"
                        } else if t_type == "volume" {
                            "分卷及其包含的章节"
                        } else {
                            "章节"
                        },
                        title
                    ));
                    if t_type == "chapter"
                        && self.state.selected_chapter_id.as_deref() == Some(c_id.as_str())
                    {
                        ui.colored_label(
                            egui::Color32::YELLOW,
                            "注意: 这是当前正在编辑的章节，删除后将关闭编辑。",
                        );
                    }

                    ui.horizontal(|ui| {
                        if ui.button("确定删除").clicked() {
                            confirm_delete = true;
                        }
                        if ui.button("取消").clicked() {
                            cancel_delete = true;
                        }
                    });
                });
        }

        if confirm_delete {
            let target = self.state.delete_target.clone().unwrap();
            let t_type = self.state.delete_target_type.clone().unwrap();

            // Force save if we are deleting currently selected chapter
            if t_type == "chapter"
                && self.state.selected_chapter_id.as_deref() == Some(target.2.as_str())
            {
                self.save_chapter();
            }

            if let Some(core) = &self.state.core {
                let mut deleted = false;
                match t_type.as_str() {
                    "project" => {
                        if let Err(e) = core.delete_project(&target.0) {
                            self.state.error_message = Some(format!("删除作品失败: {}", e));
                        } else {
                            deleted = true;
                        }
                    }
                    "volume" => {
                        if let Err(e) = core.delete_volume(&target.0, &target.1) {
                            self.state.error_message = Some(format!("删除分卷失败: {}", e));
                        } else {
                            deleted = true;
                        }
                    }
                    "chapter" => {
                        if let Err(e) = core.delete_chapter(&target.0, &target.1, &target.2) {
                            self.state.error_message = Some(format!("删除章节失败: {}", e));
                        } else {
                            deleted = true;
                        }
                    }
                    _ => {}
                }

                if deleted {
                    // Clear editor if the deleted item contains the selected chapter
                    let mut clear_editor = false;
                    if t_type == "project"
                        && self.state.selected_project_id.as_deref() == Some(target.0.as_str())
                    {
                        clear_editor = true;
                    } else if t_type == "volume"
                        && self.state.selected_volume_id.as_deref() == Some(target.1.as_str())
                    {
                        clear_editor = true;
                    } else if t_type == "chapter"
                        && self.state.selected_chapter_id.as_deref() == Some(target.2.as_str())
                    {
                        clear_editor = true;
                    }

                    if clear_editor {
                        self.state.selected_project_id = None;
                        self.state.selected_volume_id = None;
                        self.state.selected_chapter_id = None;
                        self.state.selected_chapter_title = None;
                        self.state.chapter_content.clear();
                        self.state.last_content.clear();
                        self.state.last_edit_time = None;
                        self.state.save_message = None;
                    }
                }
            }
            // Execute reload functions without holding onto `core` borrow inside
            if self.state.error_message.is_none() {
                let target = self.state.delete_target.clone().unwrap();
                let t_type = self.state.delete_target_type.clone().unwrap();
                match t_type.as_str() {
                    "project" => {
                        self.reload_projects();
                    }
                    "volume" => {
                        self.state.cached_volumes.remove(&target.0);
                        self.ensure_volumes_loaded(&target.0);
                    }
                    "chapter" => {
                        let key = (target.0.clone(), target.1.clone());
                        self.state.cached_chapters.remove(&key);
                        self.ensure_chapters_loaded(&target.0, &target.1);
                    }
                    _ => {}
                }
            }

            self.state.delete_target = None;
            self.state.delete_target_type = None;
        }

        if cancel_delete {
            self.state.delete_target = None;
            self.state.delete_target_type = None;
        }

        if self.state.workspace_path.is_some() {
            let mut chapter_to_load = None;
            let mut project_to_expand = None;
            let mut volume_to_expand = None;

            egui::SidePanel::left("left_panel")
                .resizable(true)
                .default_width(250.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("作品列表");
                        if ui.button("+").on_hover_text("新建作品").clicked() {
                            self.state.show_new_project_input = !self.state.show_new_project_input;
                        }
                    });

                    if self.state.show_new_project_input {
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut self.state.new_project_name);
                            if ui.button("确定").clicked() {
                                if let Some(core) = &self.state.core {
                                    if !self.state.new_project_name.is_empty() {
                                        match core.create_project(&self.state.new_project_name) {
                                            Ok(_) => {
                                                self.reload_projects();
                                                self.state.new_project_name.clear();
                                                self.state.show_new_project_input = false;
                                            }
                                            Err(e) => {
                                                self.state.error_message =
                                                    Some(format!("创建作品失败: {}", e));
                                            }
                                        }
                                    }
                                }
                            }
                        });
                        ui.separator();
                    }

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        // We clone the projects list to avoid borrow checker issues
                        let projects = self.state.projects.clone();
                        let mut reload_proj = false;
                        for (i, project) in projects.iter().enumerate() {
                            let p_id = project.id.clone();
                            let p_title = project.title.clone();
                            let is_p_selected =
                                self.state.selected_project_id.as_deref() == Some(&p_id);

                            ui.horizontal(|ui| {
                                let response =
                                    ui.selectable_label(is_p_selected, format!("📖 {}", p_title));
                                if response.clicked() {
                                    self.state.selected_project_id = Some(p_id.clone());
                                    project_to_expand = Some(p_id.clone());
                                }
                                if is_p_selected {
                                    if ui.button("+").on_hover_text("新建分卷").clicked() {
                                        self.state.show_new_volume_input =
                                            !self.state.show_new_volume_input;
                                    }

                                    ui.menu_button("⋮", |ui| {
                                        if ui.button("重命名").clicked() {
                                            self.save_chapter();
                                            self.state.rename_target = Some((p_id.clone(), "".to_string(), "".to_string()));
                                            self.state.rename_new_name = p_title.clone();
                                            ui.close_menu();
                                        }
                                        if ui.button("删除").clicked() {
                                            self.state.delete_target = Some((p_id.clone(), "".to_string(), "".to_string(), p_title.clone()));
                                            self.state.delete_target_type = Some("project".to_string());
                                            ui.close_menu();
                                        }
                                        if i > 0 && ui.button("上移").clicked() {
                                            self.save_chapter();
                                            if let Some(core) = &self.state.core {
                                                let mut new_order: Vec<String> = projects.iter().map(|p| p.id.clone()).collect();
                                                new_order.swap(i, i - 1);
                                                if let Err(e) = core.reorder_projects(&new_order) {
                                                    self.state.error_message = Some(format!("上移作品失败: {}", e));
                                                } else {
                                                    reload_proj = true;
                                                }
                                            }
                                            ui.close_menu();
                                        }
                                        if i < projects.len() - 1 && ui.button("下移").clicked() {
                                            self.save_chapter();
                                            if let Some(core) = &self.state.core {
                                                let mut new_order: Vec<String> = projects.iter().map(|p| p.id.clone()).collect();
                                                new_order.swap(i, i + 1);
                                                if let Err(e) = core.reorder_projects(&new_order) {
                                                    self.state.error_message = Some(format!("下移作品失败: {}", e));
                                                } else {
                                                    reload_proj = true;
                                                }
                                            }
                                            ui.close_menu();
                                        }
                                    });
                                }
                            });

                            if Some(&(p_id.clone(), "".to_string(), "".to_string())) == self.state.rename_target.as_ref() {
                                ui.horizontal(|ui| {
                                    ui.add_space(20.0);
                                    ui.text_edit_singleline(&mut self.state.rename_new_name);
                                    if ui.button("保存").clicked() {
                                        if let Some(core) = &self.state.core {
                                            if !self.state.rename_new_name.is_empty() {
                                                if let Err(e) = core.rename_project(&p_id, &self.state.rename_new_name) {
                                                    self.state.error_message = Some(format!("重命名作品失败: {}", e));
                                                } else {
                                                    reload_proj = true;
                                                }
                                            }
                                        }
                                        self.state.rename_target = None;
                                    }
                                    if ui.button("取消").clicked() {
                                        self.state.rename_target = None;
                                    }
                                });
                            }

                            if is_p_selected && self.state.show_new_volume_input {
                                ui.horizontal(|ui| {
                                    ui.add_space(20.0);
                                    ui.text_edit_singleline(&mut self.state.new_volume_name);
                                    if ui.button("确定").clicked() {
                                        if let Some(core) = &self.state.core {
                                            if !self.state.new_volume_name.is_empty() {
                                                match core.create_volume(
                                                    &p_id,
                                                    &self.state.new_volume_name,
                                                ) {
                                                    Ok(_) => {
                                                        self.state.cached_volumes.remove(&p_id);
                                                        self.ensure_volumes_loaded(&p_id);
                                                        self.state.new_volume_name.clear();
                                                        self.state.show_new_volume_input = false;
                                                    }
                                                    Err(e) => {
                                                        self.state.error_message =
                                                            Some(format!("创建分卷失败: {}", e));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                });
                            }

                            if is_p_selected {
                                if let Some(volumes) = self.state.cached_volumes.get(&p_id) {
                                    let volumes = volumes.clone();
                                    let mut reload_vol = false;
                                    for (j, volume) in volumes.iter().enumerate() {
                                        let v_id = volume.id.clone();
                                        let v_title = volume.title.clone();
                                        let is_v_selected =
                                            self.state.selected_volume_id.as_deref() == Some(&v_id);

                                        ui.horizontal(|ui| {
                                            ui.add_space(20.0);
                                            if ui
                                                .selectable_label(
                                                    is_v_selected,
                                                    format!("📚 {}", v_title),
                                                )
                                                .clicked()
                                            {
                                                self.state.selected_volume_id = Some(v_id.clone());
                                                volume_to_expand =
                                                    Some((p_id.clone(), v_id.clone()));
                                            }
                                            if is_v_selected {
                                                if ui
                                                    .button("+")
                                                    .on_hover_text("新建章节")
                                                    .clicked()
                                                {
                                                    self.state.show_new_chapter_input =
                                                        !self.state.show_new_chapter_input;
                                                }

                                                ui.menu_button("⋮", |ui| {
                                                    if ui.button("重命名").clicked() {
                                                        self.save_chapter();
                                                        self.state.rename_target = Some((p_id.clone(), v_id.clone(), "".to_string()));
                                                        self.state.rename_new_name = v_title.clone();
                                                        ui.close_menu();
                                                    }
                                                    if ui.button("删除").clicked() {
                                                        self.state.delete_target = Some((p_id.clone(), v_id.clone(), "".to_string(), v_title.clone()));
                                                        self.state.delete_target_type = Some("volume".to_string());
                                                        ui.close_menu();
                                                    }
                                                    if j > 0 && ui.button("上移").clicked() {
                                                        self.save_chapter();
                                                        if let Some(core) = &self.state.core {
                                                            let mut new_order: Vec<String> = volumes.iter().map(|v| v.id.clone()).collect();
                                                            new_order.swap(j, j - 1);
                                                            if let Err(e) = core.reorder_volumes(&p_id, &new_order) {
                                                                self.state.error_message = Some(format!("上移分卷失败: {}", e));
                                                            } else {
                                                                reload_vol = true;
                                                            }
                                                        }
                                                        ui.close_menu();
                                                    }
                                                    if j < volumes.len() - 1 && ui.button("下移").clicked() {
                                                        self.save_chapter();
                                                        if let Some(core) = &self.state.core {
                                                            let mut new_order: Vec<String> = volumes.iter().map(|v| v.id.clone()).collect();
                                                            new_order.swap(j, j + 1);
                                                            if let Err(e) = core.reorder_volumes(&p_id, &new_order) {
                                                                self.state.error_message = Some(format!("下移分卷失败: {}", e));
                                                            } else {
                                                                reload_vol = true;
                                                            }
                                                        }
                                                        ui.close_menu();
                                                    }
                                                });
                                            }
                                        });

                                        if Some(&(p_id.clone(), v_id.clone(), "".to_string())) == self.state.rename_target.as_ref() {
                                            ui.horizontal(|ui| {
                                                ui.add_space(40.0);
                                                ui.text_edit_singleline(&mut self.state.rename_new_name);
                                                if ui.button("保存").clicked() {
                                                    if let Some(core) = &self.state.core {
                                                        if !self.state.rename_new_name.is_empty() {
                                                            if let Err(e) = core.rename_volume(&p_id, &v_id, &self.state.rename_new_name) {
                                                                self.state.error_message = Some(format!("重命名分卷失败: {}", e));
                                                            } else {
                                                                reload_vol = true;
                                                            }
                                                        }
                                                    }
                                                    self.state.rename_target = None;
                                                }
                                                if ui.button("取消").clicked() {
                                                    self.state.rename_target = None;
                                                }
                                            });
                                        }

                                        if is_v_selected && self.state.show_new_chapter_input {
                                            ui.horizontal(|ui| {
                                                ui.add_space(40.0);
                                                ui.text_edit_singleline(
                                                    &mut self.state.new_chapter_name,
                                                );
                                                if ui.button("确定").clicked() {
                                                    if let Some(core) = &self.state.core {
                                                        if !self.state.new_chapter_name.is_empty() {
                                                            match core.create_chapter(
                                                                &p_id,
                                                                &v_id,
                                                                &self.state.new_chapter_name,
                                                            ) {
                                                                Ok(_) => {
                                                                    let key = (
                                                                        p_id.clone(),
                                                                        v_id.clone(),
                                                                    );
                                                                    self.state
                                                                        .cached_chapters
                                                                        .remove(&key);
                                                                    self.ensure_chapters_loaded(
                                                                        &p_id, &v_id,
                                                                    );
                                                                    self.state
                                                                        .new_chapter_name
                                                                        .clear();
                                                                    self.state
                                                                        .show_new_chapter_input =
                                                                        false;
                                                                }
                                                                Err(e) => {
                                                                    self.state.error_message =
                                                                        Some(format!(
                                                                            "创建章节失败: {}",
                                                                            e
                                                                        ));
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            });
                                        }

                                        if is_v_selected {
                                            let key = (p_id.clone(), v_id.clone());
                                            if let Some(chapters) =
                                                self.state.cached_chapters.get(&key)
                                            {
                                                let chapters = chapters.clone();
                                                let mut reload_chap = false;
                                                for (k, chapter) in chapters.iter().enumerate() {
                                                    let c_id = chapter.id.clone();
                                                    let c_title = chapter.title.clone();
                                                    let is_c_selected =
                                                        self.state.selected_chapter_id.as_deref()
                                                            == Some(&c_id);

                                                    ui.horizontal(|ui| {
                                                        ui.add_space(40.0);
                                                        if ui
                                                            .selectable_label(
                                                                is_c_selected,
                                                                format!("📄 {}", c_title),
                                                            )
                                                            .clicked()
                                                        {
                                                            chapter_to_load = Some((
                                                                p_id.clone(),
                                                                v_id.clone(),
                                                                c_id.clone(),
                                                                c_title.clone(),
                                                            ));
                                                        }

                                                        ui.menu_button("⋮", |ui| {
                                                            if ui.button("重命名").clicked() {
                                                                self.save_chapter();
                                                                self.state.rename_target = Some((p_id.clone(), v_id.clone(), c_id.clone()));
                                                                self.state.rename_new_name = c_title.clone();
                                                                ui.close_menu();
                                                            }
                                                            if ui.button("删除").clicked() {
                                                                self.state.delete_target = Some((p_id.clone(), v_id.clone(), c_id.clone(), c_title.clone()));
                                                                self.state.delete_target_type = Some("chapter".to_string());
                                                                ui.close_menu();
                                                            }
                                                            if k > 0 && ui.button("上移").clicked() {
                                                                self.save_chapter();
                                                                if let Some(core) = &self.state.core {
                                                                    let mut new_order: Vec<String> = chapters.iter().map(|c| c.id.clone()).collect();
                                                                    new_order.swap(k, k - 1);
                                                                    if let Err(e) = core.reorder_chapters(&p_id, &v_id, &new_order) {
                                                                        self.state.error_message = Some(format!("上移章节失败: {}", e));
                                                                    } else {
                                                                        reload_chap = true;
                                                                    }
                                                                }
                                                                ui.close_menu();
                                                            }
                                                            if k < chapters.len() - 1 && ui.button("下移").clicked() {
                                                                self.save_chapter();
                                                                if let Some(core) = &self.state.core {
                                                                    let mut new_order: Vec<String> = chapters.iter().map(|c| c.id.clone()).collect();
                                                                    new_order.swap(k, k + 1);
                                                                    if let Err(e) = core.reorder_chapters(&p_id, &v_id, &new_order) {
                                                                        self.state.error_message = Some(format!("下移章节失败: {}", e));
                                                                    } else {
                                                                        reload_chap = true;
                                                                    }
                                                                }
                                                                ui.close_menu();
                                                            }
                                                        });
                                                    });

                                                    if Some(&(p_id.clone(), v_id.clone(), c_id.clone())) == self.state.rename_target.as_ref() {
                                                        ui.horizontal(|ui| {
                                                            ui.add_space(60.0);
                                                            ui.text_edit_singleline(&mut self.state.rename_new_name);
                                                            if ui.button("保存").clicked() {
                                                                if let Some(core) = &self.state.core {
                                                                    if !self.state.rename_new_name.is_empty() {
                                                                        if let Err(e) = core.rename_chapter(&p_id, &v_id, &c_id, &self.state.rename_new_name) {
                                                                            self.state.error_message = Some(format!("重命名章节失败: {}", e));
                                                                        } else {
                                                                            reload_chap = true;
                                                                            if is_c_selected {
                                                                                self.state.selected_chapter_title = Some(self.state.rename_new_name.clone());
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                self.state.rename_target = None;
                                                            }
                                                            if ui.button("取消").clicked() {
                                                                self.state.rename_target = None;
                                                            }
                                                        });
                                                    }
                                                }
                                                if reload_chap {
                                                    self.state.cached_chapters.remove(&key);
                                                    self.ensure_chapters_loaded(&p_id, &v_id);
                                                }
                                            }
                                        }
                                    }
                                    if reload_vol {
                                        self.state.cached_volumes.remove(&p_id);
                                        self.ensure_volumes_loaded(&p_id);
                                    }
                                }
                            }
                        }

                        if reload_proj {
                            self.reload_projects();
                        }
                    });
                });

            if let Some(p_id) = project_to_expand {
                self.ensure_volumes_loaded(&p_id);
            }
            if let Some((p_id, v_id)) = volume_to_expand {
                self.ensure_chapters_loaded(&p_id, &v_id);
            }

            if let Some((p_id, v_id, c_id, c_title)) = chapter_to_load {
                self.load_chapter(&p_id, &v_id, &c_id, &c_title);
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(title) = &self.state.selected_chapter_title {
                    ui.heading(format!("章节: {}", title));

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let response = ui.add_sized(
                            ui.available_size(),
                            egui::TextEdit::multiline(&mut self.state.chapter_content)
                                .font(egui::TextStyle::Body)
                                .desired_width(f32::INFINITY)
                                .lock_focus(true),
                        );

                        // Delayed auto-save logic
                        if response.changed() {
                            self.state.save_message = Some("未保存".to_string());
                            self.state.last_edit_time = Some(std::time::Instant::now());
                        }
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.heading("选择章节开始写作");
                    });
                }
            });

            // Perform auto-save if 1.5 seconds have passed since the last edit
            if let Some(last_time) = self.state.last_edit_time {
                if last_time.elapsed().as_secs_f32() > 1.5 {
                    self.save_chapter();
                    self.state.last_edit_time = None;
                }
            }
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.heading("请打开或创建工作区");
                });
            });
        }

        // Request a repaint to ensure the auto-save timer is checked even if there's no UI interaction
        if self.state.last_edit_time.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }
}
