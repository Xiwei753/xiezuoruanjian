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
                        for project in &projects {
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
                                }
                            });

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
                                    for volume in volumes {
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
                                            }
                                        });

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
                                                for chapter in chapters {
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
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
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
