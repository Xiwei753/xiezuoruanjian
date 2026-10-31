// =============================================================================
// inventory.rs — Agent CLI 发现结果的数据模型
// =============================================================================
//
// Issue #1022：只保留 OpenCode 与 Codex 两个官方机器接口。这里描述本机
// 真实安装路径、版本探测和用户选择的启动路径；QML 只消费此 JSON，不写死
// 安装目录。
// =============================================================================

use serde::{Deserialize, Serialize};

/// 一个真实存在的本机可执行文件。
///
/// `launch_path` 是实际被找到的路径（可能是 npm / pipx / cargo 的 shim），
/// `canonical_path` 只用于去重，不能替换 launch path。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredExecutable {
    pub executable_name: String,
    pub launch_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_path: Option<String>,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl DiscoveredExecutable {
    pub fn new(executable_name: String, launch_path: String, source: impl Into<String>) -> Self {
        let canonical_path = std::fs::canonicalize(&launch_path)
            .ok()
            .map(|path| path.to_string_lossy().into_owned());
        Self {
            executable_name,
            launch_path,
            canonical_path,
            source: source.into(),
            package_id: None,
            version: None,
        }
    }

    /// 去重键：优先真实目标，退回归一化后的启动路径。
    pub fn dedupe_key(&self) -> String {
        self.canonical_path
            .clone()
            .unwrap_or_else(|| self.launch_path.clone())
    }
}

/// 内置 Agent 描述。只由本程序的 OpenCode / Codex 适配器提供。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDescriptor {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `acp`（OpenCode）或 `server`（Codex app-server）。
    pub protocol: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub launch: AgentLaunch,
    #[serde(default)]
    pub version_args: Vec<String>,
    #[serde(default)]
    pub login_args: Vec<String>,
    #[serde(default)]
    pub config_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// 固定为 `builtin`。
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLaunch {
    /// 可执行文件名（PATH 中的 basename），不是固定安装目录。
    #[serde(default)]
    pub bin_names: Vec<String>,
    /// 启动参数数组。ACP Agent 通常是 `acp`。
    #[serde(default)]
    pub args: Vec<String>,
    /// 目录中的发行包名（如 npx package），仅作说明，不自动执行安装。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

fn package_name(package: &str) -> &str {
    match package.rfind('@') {
        Some(index) if index > 0 => &package[..index],
        _ => package,
    }
}

impl AgentDescriptor {
    pub fn is_acp(&self) -> bool {
        self.protocol == "acp"
            || self
                .capabilities
                .iter()
                .any(|capability| capability == "acp")
    }

    pub fn matches_executable(&self, executable: &DiscoveredExecutable) -> bool {
        if self
            .launch
            .bin_names
            .iter()
            .any(|name| name == &executable.executable_name)
        {
            return true;
        }
        // npm 的 bin 名不一定等于包名（例如带 scope 的 ACP wrapper）。
        // 包管理器给出的包标识同样是“本机实际安装”的证据。
        match (
            self.launch.package.as_deref(),
            executable.package_id.as_deref(),
        ) {
            (Some(declared), Some(actual)) => package_name(declared) == actual,
            _ => false,
        }
    }
}

/// 每个候选可执行文件的探测结果。未确认的字段保持 `unknown`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProbe {
    pub version_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub config_status: String,
    pub auth_status: String,
    pub launchability: String,
    /// 该候选是否满足内置适配器的兼容性要求：`supported` /
    /// `unsupported` / `unknown`。OpenCode 只有确认主版本为 2 才算
    /// supported；界面和启动选择不能从展示用版本字符串反推。
    #[serde(default = "default_compatibility")]
    pub compatibility: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn default_compatibility() -> String {
    "unknown".to_string()
}

impl Default for AgentProbe {
    fn default() -> Self {
        Self {
            version_status: "unknown".to_string(),
            version: None,
            config_status: "unknown".to_string(),
            auth_status: "unknown".to_string(),
            launchability: "unknown".to_string(),
            compatibility: default_compatibility(),
            error: None,
        }
    }
}

impl AgentProbe {
    pub fn is_supported(&self) -> bool {
        self.compatibility == "supported"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCandidate {
    #[serde(flatten)]
    pub executable: DiscoveredExecutable,
    pub probe: AgentProbe,
}

/// 已与本机实际可执行文件匹配的 Agent。candidates 保留多个安装（例如
/// 系统包 + npm 全局），由用户选择实际执行路径。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstallation {
    pub descriptor: AgentDescriptor,
    pub candidates: Vec<AgentCandidate>,
    pub selected_launch_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInventory {
    pub generation: u64,
    pub scanned_at: String,
    pub installed: Vec<AgentInstallation>,
    /// 真实存在但不满足兼容性要求的候选（例如 OpenCode 1.x）。它们不能
    /// 被选择或启动，只用于向用户清楚报告检测到的路径与版本。
    #[serde(default)]
    pub rejected: Vec<AgentCandidate>,
}

impl AgentInventory {
    pub fn empty(generation: u64) -> Self {
        Self {
            generation,
            scanned_at: String::new(),
            installed: Vec::new(),
            rejected: Vec::new(),
        }
    }

    pub fn find_installation(&self, agent_id: &str) -> Option<&AgentInstallation> {
        self.installed
            .iter()
            .find(|installation| installation.descriptor.id == agent_id)
    }
}
