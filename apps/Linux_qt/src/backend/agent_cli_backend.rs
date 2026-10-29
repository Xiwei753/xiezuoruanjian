// =============================================================================
// agent_cli_backend.rs — Agent CLI 管理的 QObject 后端
// =============================================================================
//
// Issue #1014：扫描在后台 worker 线程进行，一次扫描只发布一个带 generation
// 的快照；旧扫描迟到的结果不能覆盖新扫描。QML 只读取 JSON 快照并通过显式
// 方法选择 Agent / 导入路径 / 打开文档与登录。
// =============================================================================

use qmetaobject::prelude::*;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use crate::agent_cli::{self, AgentCliPreferences, AgentInstallation, AgentInventory, ScanOutcome};

struct ScanResult {
    generation: u64,
    outcome: Option<ScanOutcome>,
    error: Option<String>,
}

enum ScanCommand {
    Refresh {
        generation: u64,
        reply: Box<dyn FnOnce(ScanResult) + Send>,
    },
    Shutdown,
}

#[derive(Default)]
struct AgentCliBridge {
    tx: Mutex<Option<Sender<ScanCommand>>>,
    generation: Arc<AtomicU64>,
}

impl AgentCliBridge {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel::<ScanCommand>();
        let spawn_result = std::thread::Builder::new()
            .name("linux-qt-agent-cli-scan".to_string())
            .spawn(move || worker_loop(rx));
        if let Err(error) = spawn_result {
            log::error!("failed to start Agent CLI scan worker: {error}");
        }
        Self {
            tx: Mutex::new(Some(tx)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    fn begin_scan(
        &self,
        reply: Box<dyn FnOnce(ScanResult) + Send>,
    ) -> Option<(u64, Result<(), String>)> {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let send_result = match self.tx.lock() {
            Ok(guard) => guard
                .as_ref()
                .ok_or_else(|| "Agent CLI 扫描线程已退出".to_string())
                .and_then(|tx| {
                    tx.send(ScanCommand::Refresh { generation, reply })
                        .map_err(|_| "Agent CLI 扫描线程已退出".to_string())
                }),
            Err(poisoned) => poisoned
                .into_inner()
                .as_ref()
                .ok_or_else(|| "Agent CLI 扫描线程已退出".to_string())
                .and_then(|tx| {
                    tx.send(ScanCommand::Refresh { generation, reply })
                        .map_err(|_| "Agent CLI 扫描线程已退出".to_string())
                }),
        };
        Some((generation, send_result))
    }

    fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }
}

impl Drop for AgentCliBridge {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.tx.lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(ScanCommand::Shutdown);
            }
        }
    }
}

fn worker_loop(receiver: mpsc::Receiver<ScanCommand>) {
    while let Ok(command) = receiver.recv() {
        match command {
            ScanCommand::Refresh { generation, reply } => {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    agent_cli::scan(generation)
                }));
                let reply_result = match result {
                    Ok(outcome) => ScanResult {
                        generation,
                        outcome: Some(outcome),
                        error: None,
                    },
                    Err(_) => ScanResult {
                        generation,
                        outcome: None,
                        error: Some("Agent CLI 扫描异常终止".to_string()),
                    },
                };
                reply(reply_result);
            }
            ScanCommand::Shutdown => break,
        }
    }
}

/// QML 可见的 Agent CLI 后端。
#[allow(non_snake_case)]
#[derive(QObject)]
pub struct AgentCliBackend {
    base: qt_base_class!(trait QObject),
    inventory_json: qt_property!(QString; READ inventory_json NOTIFY inventory_changed),
    scanning: qt_property!(bool; READ scanning NOTIFY scanning_changed),
    selected_agent_id: qt_property!(QString; READ selected_agent_id NOTIFY selected_agent_changed),
    selected_launch_path: qt_property!(QString; READ selected_launch_path NOTIFY selected_agent_changed),
    last_error: qt_property!(QString; READ last_error NOTIFY last_error_changed),

    inventory_changed: qt_signal!(),
    scanning_changed: qt_signal!(),
    selected_agent_changed: qt_signal!(),
    last_error_changed: qt_signal!(),

    refresh_inventory: qt_method!(fn(&mut self)),
    select_agent: qt_method!(fn(&mut self, agent_id: QString, launch_path: QString)),
    add_custom_executable: qt_method!(fn(&mut self, path: QString)),
    add_agent_manifest: qt_method!(fn(&mut self, path: QString)),
    open_agent_docs: qt_method!(fn(&mut self, agent_id: QString)),
    open_agent_login: qt_method!(fn(&mut self, agent_id: QString)),

    bridge: Arc<AgentCliBridge>,
    inventory: AgentInventory,
    preferences: AgentCliPreferences,
    inventory_json_value: String,
    scanning_value: bool,
    selected_agent_id_value: String,
    selected_launch_path_value: String,
    last_error_value: String,
}

impl AgentCliBackend {
    pub fn new() -> Self {
        let preferences = agent_cli::preferences::load();
        let inventory = AgentInventory::empty(0);
        let inventory_json_value =
            serde_json::to_string(&inventory).unwrap_or_else(|_| "{}".to_string());
        Self {
            base: Default::default(),
            inventory_json: Default::default(),
            scanning: Default::default(),
            selected_agent_id: Default::default(),
            selected_launch_path: Default::default(),
            last_error: Default::default(),
            inventory_changed: Default::default(),
            scanning_changed: Default::default(),
            selected_agent_changed: Default::default(),
            last_error_changed: Default::default(),
            refresh_inventory: Default::default(),
            select_agent: Default::default(),
            add_custom_executable: Default::default(),
            add_agent_manifest: Default::default(),
            open_agent_docs: Default::default(),
            open_agent_login: Default::default(),
            bridge: Arc::new(AgentCliBridge::new()),
            inventory,
            preferences: preferences.clone(),
            inventory_json_value,
            scanning_value: false,
            selected_agent_id_value: preferences.selected_agent_id,
            selected_launch_path_value: preferences.selected_launch_path,
            last_error_value: String::new(),
        }
    }

    fn inventory_json(&self) -> QString {
        self.inventory_json_value.clone().into()
    }

    fn scanning(&self) -> bool {
        self.scanning_value
    }

    fn selected_agent_id(&self) -> QString {
        self.selected_agent_id_value.clone().into()
    }

    fn selected_launch_path(&self) -> QString {
        self.selected_launch_path_value.clone().into()
    }

    fn last_error(&self) -> QString {
        self.last_error_value.clone().into()
    }

    fn set_scanning(&mut self, scanning: bool) {
        if self.scanning_value == scanning {
            return;
        }
        self.scanning_value = scanning;
        self.scanning_changed();
    }

    fn set_last_error(&mut self, message: String) {
        if self.last_error_value == message {
            return;
        }
        self.last_error_value = message;
        self.last_error_changed();
    }

    fn refresh_inventory(&mut self) {
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |result: ScanResult| {
            if let Some(pinned) = qptr.as_pinned() {
                let mut this = pinned.borrow_mut();
                this.apply_scan(result);
            }
        });
        self.set_scanning(true);
        match self.bridge.begin_scan(Box::new(callback)) {
            Some((_generation, Ok(()))) => {}
            Some((_generation, Err(error))) => {
                self.set_scanning(false);
                self.set_last_error(error);
            }
            None => {
                self.set_scanning(false);
                self.set_last_error("Agent CLI 扫描线程不可用".to_string());
            }
        }
    }

    fn apply_scan(&mut self, result: ScanResult) {
        if !self.bridge.is_current(result.generation) {
            // 更新的扫描已经开始；这次结果不能覆盖它。
            return;
        }
        let Some(outcome) = result.outcome else {
            if result.error.is_some() {
                self.set_scanning(false);
                self.set_last_error(result.error.unwrap_or_default());
            }
            return;
        };

        let ScanOutcome {
            inventory,
            preferences: _,
        } = outcome;
        let selected = choose_selection(
            &inventory,
            &self.selected_agent_id_value,
            &self.selected_launch_path_value,
            &self.preferences.selected_agent_id,
            &self.preferences.selected_launch_path,
        );
        let mut error = String::new();
        let selection_changed = self.selected_agent_id_value != selected.0
            || self.selected_launch_path_value != selected.1;
        if selection_changed {
            self.selected_agent_id_value = selected.0.clone();
            self.selected_launch_path_value = selected.1.clone();
            self.preferences.selected_agent_id = selected.0;
            self.preferences.selected_launch_path = selected.1;
            if let Err(save_error) = agent_cli::preferences::save(&self.preferences) {
                error = save_error;
            }
        }

        self.inventory = inventory;
        self.inventory_json_value =
            serde_json::to_string(&self.inventory).unwrap_or_else(|_| "{}".to_string());
        self.inventory_changed();
        if selection_changed {
            self.selected_agent_changed();
        }
        self.last_error_value = error;
        self.last_error_changed();
        self.set_scanning(false);
    }

    fn select_agent(&mut self, agent_id: QString, launch_path: QString) {
        let agent_id = agent_id.to_string();
        let launch_path = launch_path.to_string();
        let Some(installation) = self.inventory.find_installation(&agent_id) else {
            self.set_last_error("只能选择本机已经检测到的 Agent".to_string());
            return;
        };
        if !installation
            .candidates
            .iter()
            .any(|candidate| candidate.executable.launch_path == launch_path)
        {
            self.set_last_error("所选启动路径不属于该 Agent 的本机安装".to_string());
            return;
        }
        if self.selected_agent_id_value == agent_id
            && self.selected_launch_path_value == launch_path
        {
            return;
        }
        let selected_id = agent_id.clone();
        self.selected_agent_id_value = agent_id.clone();
        self.selected_launch_path_value = launch_path.clone();
        self.preferences.selected_agent_id = agent_id;
        self.preferences.selected_launch_path = launch_path.clone();
        if let Some(installation) = self
            .inventory
            .installed
            .iter_mut()
            .find(|installation| installation.descriptor.id == selected_id)
        {
            installation.selected_launch_path = launch_path;
        }
        self.inventory_json_value =
            serde_json::to_string(&self.inventory).unwrap_or_else(|_| "{}".to_string());
        self.inventory_changed();
        self.selected_agent_changed();
        if let Err(error) = agent_cli::preferences::save(&self.preferences) {
            self.set_last_error(error);
        } else {
            self.set_last_error(String::new());
        }
    }

    fn add_custom_executable(&mut self, path: QString) {
        let path = path.to_string();
        if path.is_empty() {
            self.set_last_error("可执行文件路径为空".to_string());
            return;
        }
        // 只规范化为绝对路径，不解析符号链接：npm/pipx/cargo 的 shim 往往
        // 必须通过自身路径启动，canonical target 只应在发现阶段用于去重。
        let path = std::path::absolute(&path).unwrap_or_else(|_| std::path::PathBuf::from(&path));
        let executable = path.is_file()
            && std::fs::metadata(&path)
                .map(|metadata| {
                    use std::os::unix::fs::PermissionsExt;
                    metadata.permissions().mode() & 0o111 != 0
                })
                .unwrap_or(false);
        if !executable {
            self.set_last_error(format!("可执行文件不存在或不可执行：{}", path.display()));
            return;
        }
        let path = path.to_string_lossy().into_owned();
        if !self.preferences.custom_executables.contains(&path) {
            self.preferences.custom_executables.push(path);
            if let Err(error) = agent_cli::preferences::save(&self.preferences) {
                self.set_last_error(error);
                return;
            }
        }
        self.set_last_error(String::new());
        self.refresh_inventory();
    }

    fn add_agent_manifest(&mut self, path: QString) {
        let path = path.to_string();
        let manifest = std::path::PathBuf::from(&path);
        match agent_cli::registry::load_manifest(&manifest) {
            Ok(agents) if !agents.is_empty() => {
                let path = manifest.to_string_lossy().into_owned();
                if !self.preferences.agent_manifests.contains(&path) {
                    self.preferences.agent_manifests.push(path);
                    if let Err(error) = agent_cli::preferences::save(&self.preferences) {
                        self.set_last_error(error);
                        return;
                    }
                }
                self.set_last_error(String::new());
                self.refresh_inventory();
            }
            Ok(_) => self.set_last_error("manifest 未声明任何 Agent".to_string()),
            Err(error) => self.set_last_error(error),
        }
    }

    fn open_agent_docs(&mut self, agent_id: QString) {
        let agent_id = agent_id.to_string();
        let url = self
            .inventory
            .find_installation(&agent_id)
            .map(|installation| &installation.descriptor)
            .or_else(|| {
                self.inventory
                    .available
                    .iter()
                    .find(|descriptor| descriptor.id == agent_id)
            })
            .and_then(|descriptor| {
                descriptor
                    .docs_url
                    .clone()
                    .or_else(|| descriptor.website.clone())
                    .or_else(|| descriptor.repository.clone())
            });
        let Some(url) = url else {
            self.set_last_error("该 Agent 没有声明文档地址".to_string());
            return;
        };
        let mut attempts = vec![
            ("xdg-open", vec![url.clone()]),
            ("gio", vec!["open".to_string(), url]),
        ];
        for (program, args) in attempts.drain(..) {
            let spawned = Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            if spawned.is_ok() {
                self.set_last_error(String::new());
                return;
            }
        }
        self.set_last_error("无法打开文档：未找到 xdg-open 或 gio".to_string());
    }

    fn open_agent_login(&mut self, agent_id: QString) {
        let agent_id = agent_id.to_string();
        let Some(installation) = self.inventory.find_installation(&agent_id) else {
            self.set_last_error("该 Agent 尚未安装到本机".to_string());
            return;
        };
        let launch_path = if installation.selected_launch_path.is_empty() {
            installation
                .candidates
                .first()
                .map(|candidate| candidate.executable.launch_path.clone())
                .unwrap_or_default()
        } else {
            installation.selected_launch_path.clone()
        };
        if launch_path.is_empty() {
            self.set_last_error("该 Agent 没有可用的启动路径".to_string());
            return;
        }
        let login_args = installation.descriptor.login_args.clone();
        let current_dir = std::env::current_dir().ok();
        let mut attempts: Vec<(&str, Vec<String>)> = vec![
            ("xdg-terminal-exec", Vec::new()),
            ("konsole", vec!["-e".to_string()]),
            ("gnome-terminal", vec!["--".to_string()]),
            ("x-terminal-emulator", vec!["-e".to_string()]),
            ("kgx", vec!["--".to_string()]),
            ("xterm", vec!["-e".to_string()]),
        ];
        for (program, terminal_args) in attempts.drain(..) {
            let mut command = Command::new(program);
            command
                .args(terminal_args)
                .arg(&launch_path)
                .args(&login_args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if let Some(directory) = current_dir.as_ref() {
                command.current_dir(directory);
            }
            if command.spawn().is_ok() {
                self.set_last_error(String::new());
                return;
            }
        }
        self.set_last_error("无法打开终端执行 Agent 登录".to_string());
    }
}

impl Default for AgentCliBackend {
    fn default() -> Self {
        Self::new()
    }
}

fn choose_selection(
    inventory: &AgentInventory,
    current_id: &str,
    current_path: &str,
    preferred_id: &str,
    preferred_path: &str,
) -> (String, String) {
    for (agent_id, launch_path) in [(current_id, current_path), (preferred_id, preferred_path)] {
        if agent_id.is_empty() {
            continue;
        }
        let Some(installation) = inventory.find_installation(agent_id) else {
            continue;
        };
        let path = if installation
            .candidates
            .iter()
            .any(|candidate| candidate.executable.launch_path == launch_path)
        {
            launch_path.to_string()
        } else {
            installation
                .candidates
                .first()
                .map(|candidate| candidate.executable.launch_path.clone())
                .unwrap_or_default()
        };
        if !path.is_empty() {
            return (agent_id.to_string(), path);
        }
    }
    inventory
        .installed
        .first()
        .and_then(|installation: &AgentInstallation| {
            installation.candidates.first().map(|candidate| {
                (
                    installation.descriptor.id.clone(),
                    candidate.executable.launch_path.clone(),
                )
            })
        })
        .unwrap_or_default()
}
