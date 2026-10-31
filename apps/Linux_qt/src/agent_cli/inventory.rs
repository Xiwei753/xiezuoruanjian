// =============================================================================
// inventory.rs — Agent CLI 发现结果的数据模型
// =============================================================================
//
// Issue #1014：Agent 不由固定枚举列举。目录（registry / 本地 manifest）中的
// Agent 是可安装条目；只有和本机真实可执行文件或包元数据匹配后，才进入
// installed 列表。QML 只消费这里定义的 JSON，不写死 Agent 数量或安装路径。
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

/// 声明式 Agent 描述。所有字段都由 registry / manifest / 内置适配器提供，
/// 不依赖固定枚举。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDescriptor {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `acp` 或 `cli`。ACP 是首选项，但协议描述由数据决定。
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
    /// `builtin` / `registry` / `manifest`
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Default for AgentProbe {
    fn default() -> Self {
        Self {
            version_status: "unknown".to_string(),
            version: None,
            config_status: "unknown".to_string(),
            auth_status: "unknown".to_string(),
            launchability: "unknown".to_string(),
            error: None,
        }
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
    pub registry_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_error: Option<String>,
}

impl AgentInventory {
    pub fn empty(generation: u64) -> Self {
        Self {
            generation,
            scanned_at: String::new(),
            installed: Vec::new(),
            registry_source: "none".to_string(),
            registry_error: None,
        }
    }

    pub fn find_installation(&self, agent_id: &str) -> Option<&AgentInstallation> {
        self.installed
            .iter()
            .find(|installation| installation.descriptor.id == agent_id)
    }
}
