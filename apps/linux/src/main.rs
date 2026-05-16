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
        "Linux Writer MVP",
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

    // Editor state
    chapter_content: String,

    // Auto-save state
    last_content: String,

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
            chapter_content: String::new(),
            last_content: String::new(),
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
                            self.state.error_message =
                                Some(format!("Failed to create workspace: {}", e));
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
                    self.state.error_message = Some(format!("Failed to load projects: {}", e));
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
                    self.state.error_message = Some(format!("Failed to read chapter: {}", e));
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
                    self.state.save_message = Some("Chapter saved successfully.".to_string());
                    self.state.last_content = self.state.chapter_content.clone();
                }
                Err(e) => {
                    self.state.error_message = Some(format!("Failed to save chapter: {}", e));
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

impl eframe::App for WriterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open / Create Workspace").clicked() {
                    self.open_workspace();
                }
                if let Some(path) = &self.state.workspace_path {
                    ui.label(format!("Workspace: {}", path.display()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(msg) = &self.state.save_message {
                        ui.label(egui::RichText::new(msg).color(egui::Color32::GREEN));
                    }
                    if self.state.selected_chapter_id.is_some() {
                        if ui.button("Save").clicked() {
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
                    if ui.button("Clear Error").clicked() {
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
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        // We clone the projects list to avoid borrow checker issues
                        let projects = self.state.projects.clone();
                        for project in &projects {
                            let p_id = project.id.clone();
                            let p_title = project.title.clone();
                            let is_p_selected =
                                self.state.selected_project_id.as_deref() == Some(&p_id);

                            let response =
                                ui.selectable_label(is_p_selected, format!("📖 {}", p_title));
                            if response.clicked() {
                                self.state.selected_project_id = Some(p_id.clone());
                                project_to_expand = Some(p_id.clone());
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
                                        });

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
                    ui.heading(format!("Chapter: {}", title));

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let response = ui.add_sized(
                            ui.available_size(),
                            egui::TextEdit::multiline(&mut self.state.chapter_content)
                                .font(egui::TextStyle::Body)
                                .desired_width(f32::INFINITY)
                                .lock_focus(true),
                        );

                        // Very simple auto-save logic on change indicator
                        if response.changed() {
                            self.state.save_message = Some("Unsaved changes...".to_string());
                            // Just save whenever it changes. Since write_chapter creates I/O we don't
                            // strictly want to do it every frame. But for MVP this guarantees auto-save.
                            self.save_chapter();
                        }
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.heading("Select a chapter to edit.");
                    });
                }
            });
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.heading("Please open a workspace");
                });
            });
        }
    }
}
