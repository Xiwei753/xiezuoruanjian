// =============================================================================
// agent_cli — Linux Qt 的 Agent CLI 动态发现与管理
// =============================================================================
//
// Issue #1014：目录中的 Agent 是数据，不是枚举；安装状态必须由本机实际
// 可执行文件/包元数据确认。扫描在后台线程完成，QML 只消费一次发布的结果。
// =============================================================================

pub mod adapters;
pub mod discovery;
pub mod inventory;
pub mod preferences;
pub mod probe;
pub mod registry;

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
    let preferences = preferences::load();
    let executables = discovery::collect_installed_executables(&preferences.custom_executables);
    let registry = registry::load_registry();

    let mut manifest_agents = Vec::new();
    let mut manifest_errors = Vec::new();
    for path in &preferences.agent_manifests {
        match registry::load_manifest(std::path::Path::new(path)) {
            Ok(mut agents) => manifest_agents.append(&mut agents),
            Err(error) => manifest_errors.push(error),
        }
    }

    let catalog = adapters::merge_catalog(
        adapters::builtin_descriptors(),
        registry.agents,
        manifest_agents,
    );

    let mut preferred_paths = HashMap::new();
    if !preferences.selected_agent_id.is_empty() && !preferences.selected_launch_path.is_empty() {
        preferred_paths.insert(
            preferences.selected_agent_id.clone(),
            preferences.selected_launch_path.clone(),
        );
    }

    let installed = adapters::build_installations(&catalog, &executables, &preferred_paths);

    let mut registry_error = registry.error;
    if !manifest_errors.is_empty() {
        let message = format!("部分本地 manifest 无法加载：{}", manifest_errors.join("；"));
        registry_error = Some(match registry_error {
            Some(existing) => format!("{existing}；{message}"),
            None => message,
        });
    }

    ScanOutcome {
        inventory: AgentInventory {
            generation,
            scanned_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            installed,
            registry_source: registry.source,
            registry_error,
        },
        preferences,
    }
}
