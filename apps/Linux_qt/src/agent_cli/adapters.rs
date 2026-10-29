// =============================================================================
// adapters.rs — 目录条目、实际可执行文件与探测结果的关联
// =============================================================================
//
// ACP Agent 优先展示。内置描述只描述已知 CLI 的真实协议、版本命令、配置位置
// 和登录入口；未知 CLI 通过用户手工路径 + 自定义 manifest 接入，不为任何
// PATH 程序伪造 `--help` 兼容性。
// =============================================================================

use std::collections::HashSet;

use super::discovery::enrich_package_metadata;
use super::inventory::{
    AgentCandidate, AgentDescriptor, AgentInstallation, AgentLaunch, DiscoveredExecutable,
};
use super::probe::probe_executable;

pub fn builtin_descriptors() -> Vec<AgentDescriptor> {
    vec![
        AgentDescriptor {
            id: "codex".to_string(),
            name: "Codex".to_string(),
            version: None,
            description: Some("OpenAI Codex CLI".to_string()),
            protocol: "cli".to_string(),
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
        AgentDescriptor {
            id: "claude".to_string(),
            name: "Claude Code".to_string(),
            version: None,
            description: Some("Anthropic Claude Code CLI".to_string()),
            protocol: "cli".to_string(),
            capabilities: vec!["chat".to_string(), "code".to_string()],
            launch: AgentLaunch {
                bin_names: vec!["claude".to_string()],
                args: Vec::new(),
                package: None,
            },
            version_args: vec!["--version".to_string()],
            // Claude Code 的登录在交互式 CLI 内完成，不伪造不存在的子命令。
            login_args: Vec::new(),
            config_paths: vec!["~/.claude/settings.json".to_string()],
            docs_url: Some("https://docs.anthropic.com/en/docs/claude-code".to_string()),
            website: Some("https://www.anthropic.com/claude-code".to_string()),
            repository: None,
            provenance: "builtin".to_string(),
        },
        AgentDescriptor {
            id: "gemini".to_string(),
            name: "Gemini CLI".to_string(),
            version: None,
            description: Some("Google Gemini CLI（ACP 模式）".to_string()),
            protocol: "acp".to_string(),
            capabilities: vec!["acp".to_string(), "chat".to_string()],
            launch: AgentLaunch {
                bin_names: vec!["gemini".to_string()],
                args: vec!["--acp".to_string()],
                package: None,
            },
            version_args: vec!["--version".to_string()],
            login_args: Vec::new(),
            config_paths: vec!["~/.gemini/settings.json".to_string()],
            docs_url: Some("https://github.com/google-gemini/gemini-cli".to_string()),
            website: Some("https://github.com/google-gemini/gemini-cli".to_string()),
            repository: Some("https://github.com/google-gemini/gemini-cli".to_string()),
            provenance: "builtin".to_string(),
        },
        AgentDescriptor {
            id: "opencode".to_string(),
            name: "OpenCode".to_string(),
            version: None,
            description: Some("OpenCode ACP Agent".to_string()),
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
            docs_url: Some("https://opencode.ai/docs".to_string()),
            website: Some("https://opencode.ai".to_string()),
            repository: Some("https://github.com/sst/opencode".to_string()),
            provenance: "builtin".to_string(),
        },
    ]
}

/// 合并内置描述、官方 registry 与用户 manifest。同一个 id 的条目合并
/// binNames / 元数据；manifest 作为用户覆盖拥有最高优先级。
pub fn merge_catalog(
    builtin: Vec<AgentDescriptor>,
    registry: Vec<AgentDescriptor>,
    manifests: Vec<AgentDescriptor>,
) -> Vec<AgentDescriptor> {
    let mut catalog = builtin;
    for descriptor in registry {
        if let Some(existing) = catalog
            .iter_mut()
            .find(|existing| existing.id == descriptor.id)
        {
            merge_descriptor(existing, descriptor);
        } else {
            catalog.push(descriptor);
        }
    }
    for descriptor in manifests {
        if let Some(existing) = catalog
            .iter_mut()
            .find(|existing| existing.id == descriptor.id)
        {
            let mut replacement = descriptor;
            for name in existing.launch.bin_names.iter() {
                if !replacement.launch.bin_names.contains(name) {
                    replacement.launch.bin_names.push(name.clone());
                }
            }
            *existing = replacement;
        } else {
            catalog.push(descriptor);
        }
    }
    catalog
}

fn merge_descriptor(existing: &mut AgentDescriptor, overlay: AgentDescriptor) {
    for name in overlay.launch.bin_names {
        if !existing.launch.bin_names.contains(&name) {
            existing.launch.bin_names.push(name);
        }
    }
    if existing.launch.args.is_empty() {
        existing.launch.args = overlay.launch.args;
    }
    if existing.launch.package.is_none() {
        existing.launch.package = overlay.launch.package;
    }
    if existing.version.is_none() {
        existing.version = overlay.version;
    }
    if existing.description.is_none() {
        existing.description = overlay.description;
    }
    if existing.docs_url.is_none() {
        existing.docs_url = overlay.docs_url;
    }
    if existing.website.is_none() {
        existing.website = overlay.website;
    }
    if existing.repository.is_none() {
        existing.repository = overlay.repository;
    }
    for capability in overlay.capabilities {
        if !existing.capabilities.contains(&capability) {
            existing.capabilities.push(capability);
        }
    }
    if existing.provenance != overlay.provenance {
        existing.provenance = format!("{}+{}", existing.provenance, overlay.provenance);
    }
}

/// 把目录条目匹配到本机实际文件。只有在这里匹配成功的条目才进入
/// installed；registry 里未被安装的条目继续留在 available。
pub fn build_installations(
    catalog: &[AgentDescriptor],
    executables: &[DiscoveredExecutable],
    preferred_paths: &std::collections::HashMap<String, String>,
) -> (Vec<AgentInstallation>, Vec<AgentDescriptor>) {
    let mut installed = Vec::new();
    let mut installed_ids = HashSet::new();

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
            let mut executable = executable.clone();
            enrich_package_metadata(&mut executable);
            let probe = probe_executable(&executable, descriptor);
            candidates.push(AgentCandidate { executable, probe });
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
        installed_ids.insert(descriptor.id.clone());
        installed.push(AgentInstallation {
            descriptor: descriptor.clone(),
            candidates,
            selected_launch_path,
        });
    }

    let mut available = catalog
        .iter()
        .filter(|descriptor| !installed_ids.contains(&descriptor.id))
        .cloned()
        .collect::<Vec<_>>();

    installed.sort_by(|left, right| {
        right
            .descriptor
            .is_acp()
            .cmp(&left.descriptor.is_acp())
            .then_with(|| left.descriptor.name.cmp(&right.descriptor.name))
    });
    available.sort_by(|left, right| {
        right
            .is_acp()
            .cmp(&left.is_acp())
            .then_with(|| left.name.cmp(&right.name))
    });

    (installed, available)
}
