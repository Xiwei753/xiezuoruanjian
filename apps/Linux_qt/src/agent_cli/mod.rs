// =============================================================================
// agent_cli — Linux Qt 的 Agent CLI 发现与选择
// =============================================================================
//
// Issue #1022：只发现 OpenCode 与 Codex 的真实安装路径。扫描在后台线程
// 完成，QML 只消费一次发布的结果。
// =============================================================================

pub mod adapters;
pub mod discovery;
pub mod inventory;
pub mod preferences;
pub mod probe;

use std::collections::HashMap;

pub use inventory::{
    AgentCandidate, AgentDescriptor, AgentInstallation, AgentInventory, AgentLaunch, AgentProbe,
    DiscoveredExecutable,
};
pub use preferences::AgentCliPreferences;

pub struct ScanOutcome {
    pub inventory: AgentInventory,
    pub preferences: AgentCliPreferences,
}

/// 一次完整扫描。只应在后台 worker 线程调用。
pub fn scan(generation: u64) -> ScanOutcome {
    let mut preferences = preferences::load();
    // 旧版本可能保存了 claude / gemini 或第三方 Agent 标识；这些标识不再
    // 对应任何内置描述，直接清掉，避免选择器残留不可执行的条目。
    if !matches!(preferences.selected_agent_id.as_str(), "opencode" | "codex") {
        preferences.selected_agent_id.clear();
        preferences.selected_launch_path.clear();
    }

    let executables = discovery::collect_installed_executables(&preferences.custom_executables);
    let catalog = adapters::builtin_descriptors();

    let mut preferred_paths = HashMap::new();
    if !preferences.selected_agent_id.is_empty() && !preferences.selected_launch_path.is_empty() {
        preferred_paths.insert(
            preferences.selected_agent_id.clone(),
            preferences.selected_launch_path.clone(),
        );
    }

    let installed = adapters::build_installations(&catalog, &executables, &preferred_paths);

    ScanOutcome {
        inventory: AgentInventory {
            generation,
            scanned_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            installed,
        },
        preferences,
    }
}
