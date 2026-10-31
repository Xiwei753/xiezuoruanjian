// =============================================================================
// adapters.rs — 内置 CLI 描述与实际可执行文件的关联
// =============================================================================
//
// Issue #1022：只接入 OpenCode（官方 ACP）与 Codex（官方 app-server）。
// 不再读取在线 ACP Registry，也不再接受任意第三方 manifest。
// =============================================================================

use std::collections::HashSet;

use super::inventory::{
    AgentCandidate, AgentDescriptor, AgentInstallation, AgentLaunch, DiscoveredExecutable,
};
use super::probe::probe_executable;

pub fn builtin_descriptors() -> Vec<AgentDescriptor> {
    vec![
        AgentDescriptor {
            id: "opencode".to_string(),
            name: "OpenCode".to_string(),
            version: None,
            description: Some("OpenCode v2 官方 ACP 模式".to_string()),
            protocol: "acp".to_string(),
            capabilities: vec!["acp".to_string(), "chat".to_string(), "code".to_string()],
            launch: AgentLaunch {
                bin_names: vec!["opencode".to_string()],
                args: vec!["acp".to_string()],
                package: None,
            },
            version_args: vec!["--version".to_string()],
            login_args: vec!["auth".to_string(), "login".to_string()],
            config_paths: vec!["~/.config/opencode/opencode.json".to_string()],
            docs_url: Some("https://opencode.ai/v2/docs/cli/acp/".to_string()),
            website: Some("https://opencode.ai/v2/docs".to_string()),
            repository: Some("https://github.com/anomalyco/opencode".to_string()),
            provenance: "builtin".to_string(),
        },
        AgentDescriptor {
            id: "codex".to_string(),
            name: "Codex".to_string(),
            version: None,
            description: Some("OpenAI Codex CLI（官方 app-server）".to_string()),
            protocol: "server".to_string(),
            capabilities: vec!["chat".to_string(), "code".to_string()],
            launch: AgentLaunch {
                bin_names: vec!["codex".to_string()],
                args: Vec::new(),
                package: None,
            },
            version_args: vec!["--version".to_string()],
            login_args: vec!["login".to_string()],
            config_paths: vec!["~/.codex/config.toml".to_string()],
            docs_url: Some("https://github.com/openai/codex".to_string()),
            website: Some("https://github.com/openai/codex".to_string()),
            repository: Some("https://github.com/openai/codex".to_string()),
            provenance: "builtin".to_string(),
        },
    ]
}

/// 把内置描述匹配到本机实际文件。只有真实安装路径才进入 installed；
/// OpenCode 还必须在探测阶段确认主版本为 2.x，1.x 或无法解析的安装只会
/// 进入 `rejected`，供界面报告，不会成为可启动候选。
pub fn build_installations(
    catalog: &[AgentDescriptor],
    executables: &[DiscoveredExecutable],
    preferred_paths: &std::collections::HashMap<String, String>,
) -> (Vec<AgentInstallation>, Vec<AgentCandidate>) {
    let mut installed = Vec::new();
    let mut rejected = Vec::new();

    for descriptor in catalog {
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        for executable in executables {
            if !descriptor.matches_executable(executable) {
                continue;
            }
            if !seen.insert(executable.dedupe_key()) {
                continue;
            }
            let executable = executable.clone();
            let probe = probe_executable(&executable, descriptor);
            let candidate = AgentCandidate { executable, probe };
            if descriptor.id == "opencode" && !candidate.probe.is_supported() {
                log::warn!(
                    "[agent_cli] 忽略不兼容的 OpenCode 安装：{}（{}）",
                    candidate.executable.launch_path,
                    candidate
                        .probe
                        .error
                        .as_deref()
                        .unwrap_or("未确认 OpenCode 2.x")
                );
                rejected.push(candidate);
                continue;
            }
            candidates.push(candidate);
        }
        if candidates.is_empty() {
            continue;
        }
        candidates.sort_by(|left, right| {
            left.executable
                .launch_path
                .cmp(&right.executable.launch_path)
        });
        let preferred = preferred_paths.get(&descriptor.id);
        let selected_launch_path = preferred
            .filter(|path| {
                candidates
                    .iter()
                    .any(|candidate| &candidate.executable.launch_path == *path)
            })
            .cloned()
            .unwrap_or_else(|| candidates[0].executable.launch_path.clone());
        installed.push(AgentInstallation {
            descriptor: descriptor.clone(),
            candidates,
            selected_launch_path,
        });
    }

    installed.sort_by(|left, right| {
        right
            .descriptor
            .is_acp()
            .cmp(&left.descriptor.is_acp())
            .then_with(|| left.descriptor.name.cmp(&right.descriptor.name))
    });
    rejected.sort_by(|left, right| {
        left.executable
            .launch_path
            .cmp(&right.executable.launch_path)
    });

    (installed, rejected)
}
