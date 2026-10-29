// =============================================================================
// registry.rs — ACP Agent 官方目录与本地 manifest
// =============================================================================
//
// 官方 registry 是“可安装目录”，不是本机安装清单。条目只有和 discovery
// 找到的真实可执行文件匹配后才标记为已安装。这里从不执行 registry 里的
// 安装命令。
// =============================================================================

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;
use serde_json::Value;

use super::inventory::{AgentDescriptor, AgentLaunch};

pub const REGISTRY_URL: &str =
    "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
const CACHE_MAX_AGE: Duration = Duration::from_secs(6 * 60 * 60);

pub struct RegistryLoad {
    pub agents: Vec<AgentDescriptor>,
    pub source: String,
    pub error: Option<String>,
}

pub fn load_registry() -> RegistryLoad {
    let cache_path = cache_path();
    if let Some(cached) = read_fresh_cache(&cache_path) {
        return RegistryLoad {
            agents: cached,
            source: "cache".to_string(),
            error: None,
        };
    }

    match fetch_network() {
        Ok(text) => match parse_registry_json(&text) {
            Ok(agents) => {
                write_cache(&cache_path, &text);
                RegistryLoad {
                    agents,
                    source: "network".to_string(),
                    error: None,
                }
            }
            Err(error) => fallback_cache(&cache_path, error),
        },
        Err(error) => fallback_cache(&cache_path, error),
    }
}

fn fallback_cache(cache_path: &Path, network_error: String) -> RegistryLoad {
    match fs::read_to_string(cache_path).map_err(|error| error.to_string()) {
        Ok(text) => match parse_registry_json(&text) {
            Ok(agents) => RegistryLoad {
                agents,
                source: "cache-fallback".to_string(),
                error: Some(format!("官方目录更新失败，使用本地缓存：{network_error}")),
            },
            Err(cache_error) => RegistryLoad {
                agents: Vec::new(),
                source: "none".to_string(),
                error: Some(format!(
                    "官方目录更新失败（{network_error}），缓存也无法解析（{cache_error}）"
                )),
            },
        },
        Err(_) => RegistryLoad {
            agents: Vec::new(),
            source: "none".to_string(),
            error: Some(format!("无法获取 Agent 目录：{network_error}")),
        },
    }
}

fn fetch_network() -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("sujian-linux-qt/0.1")
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(REGISTRY_URL)
        .send()
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    response.text().map_err(|error| error.to_string())
}

fn cache_path() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"));
    base.join("sujian").join("agent-registry.json")
}

fn read_fresh_cache(path: &Path) -> Option<Vec<AgentDescriptor>> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    if age > CACHE_MAX_AGE {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    parse_registry_json(&text).ok()
}

fn write_cache(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let temporary = path.with_extension("json.tmp");
    if fs::write(&temporary, text).is_ok() {
        let _ = fs::rename(&temporary, path);
    }
}

#[derive(Debug, Deserialize)]
struct RegistryDocument {
    #[serde(default)]
    agents: Vec<RegistryAgent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryAgent {
    id: String,
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    repository: Option<String>,
    #[serde(default)]
    website: Option<String>,
    #[serde(default)]
    distribution: Option<RegistryDistribution>,
}

#[derive(Debug, Deserialize)]
struct RegistryDistribution {
    #[serde(default)]
    npx: Option<NpxDistribution>,
    #[serde(default)]
    binary: Option<HashMap<String, BinaryDistribution>>,
}

#[derive(Debug, Deserialize)]
struct NpxDistribution {
    package: String,
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BinaryDistribution {
    #[serde(default)]
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
}

pub fn parse_registry_json(text: &str) -> Result<Vec<AgentDescriptor>, String> {
    let document: RegistryDocument =
        serde_json::from_str(text).map_err(|error| format!("目录 JSON 解析失败：{error}"))?;
    let mut agents = Vec::new();
    for agent in document.agents {
        let mut bin_names = Vec::new();
        let mut args = Vec::new();
        let mut package = None;
        let mut version = agent.version;
        if let Some(distribution) = agent.distribution {
            if let Some(npx) = distribution.npx {
                if let Some(name) = package_bin_name(&npx.package) {
                    bin_names.push(name);
                }
                if let Some((_, package_version)) = split_package_version(&npx.package) {
                    version = version.or_else(|| Some(package_version.to_string()));
                }
                package = Some(npx.package);
                args = npx.args;
            }
            if let Some(binaries) = distribution.binary {
                for binary in binaries.values() {
                    if let Some(name) = binary_bin_name(&binary.cmd) {
                        if !bin_names.contains(&name) {
                            bin_names.push(name);
                        }
                    }
                }
                // 参数必须取自当前平台；不能把 darwin/windows 的启动参数
                // 应用到 Linux 安装。
                if args.is_empty() {
                    if let Some(binary) = binaries.get(&current_registry_platform()) {
                        args = binary.args.clone();
                    }
                }
            }
        }
        bin_names.sort();
        bin_names.dedup();
        if bin_names.is_empty() {
            continue;
        }
        let docs_url = agent.website.clone().or_else(|| agent.repository.clone());
        agents.push(AgentDescriptor {
            id: agent.id,
            name: agent.name,
            version,
            description: agent.description,
            protocol: "acp".to_string(),
            capabilities: vec!["acp".to_string()],
            launch: AgentLaunch {
                bin_names,
                args,
                package,
            },
            version_args: Vec::new(),
            login_args: Vec::new(),
            config_paths: Vec::new(),
            docs_url,
            website: agent.website,
            repository: agent.repository,
            provenance: "registry".to_string(),
        });
    }
    Ok(agents)
}

fn package_bin_name(package: &str) -> Option<String> {
    let without_version = split_package_version(package)
        .map(|(name, _)| name)
        .unwrap_or(package);
    without_version
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// 分离 `pkg` / `@scope/pkg` / `pkg@1.2.3` / `@scope/pkg@1.2.3`。
fn split_package_version(package: &str) -> Option<(&str, &str)> {
    let at = package.rfind('@')?;
    if at == 0 {
        return None;
    }
    Some((&package[..at], &package[at + 1..]))
}

fn binary_bin_name(command: &str) -> Option<String> {
    let name = command.trim().trim_start_matches("./").rsplit('/').next()?;
    let name = name.strip_suffix(".exe").unwrap_or(name);
    (!name.is_empty()).then(|| name.to_string())
}

fn current_registry_platform() -> String {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestDocument {
    #[serde(default)]
    agents: Vec<ManifestAgent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestAgent {
    id: String,
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    launch: Option<AgentLaunch>,
    #[serde(default, alias = "bin_names")]
    bin_names: Vec<String>,
    #[serde(default)]
    bin: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default, alias = "version_args")]
    version_args: Vec<String>,
    #[serde(default, alias = "login_args")]
    login_args: Vec<String>,
    #[serde(default, alias = "config_paths")]
    config_paths: Vec<String>,
    #[serde(default, alias = "docs_url")]
    docs_url: Option<String>,
    #[serde(default)]
    website: Option<String>,
    #[serde(default)]
    repository: Option<String>,
}

pub fn load_manifest(path: &Path) -> Result<Vec<AgentDescriptor>, String> {
    if !path.is_file() {
        return Err(format!("manifest 不存在：{}", path.display()));
    }
    let text = fs::read_to_string(path).map_err(|error| format!("无法读取 manifest：{error}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|error| format!("manifest JSON 解析失败：{error}"))?;

    // 用户可以直接加载与官方 Registry 同格式的文件（含单个 Agent），
    // distribution 里的 npx/二进制入口继续作为描述数据，不执行安装。
    let registry_value = if value.get("agents").is_some() {
        Some(value.clone())
    } else if value.get("distribution").is_some() {
        Some(serde_json::json!({ "agents": [value.clone()] }))
    } else {
        None
    };
    if let Some(registry_value) = registry_value {
        let registry_text = serde_json::to_string(&registry_value)
            .map_err(|error| format!("manifest 重新序列化失败：{error}"))?;
        if let Ok(mut agents) = parse_registry_json(&registry_text) {
            if !agents.is_empty() {
                for agent in &mut agents {
                    agent.provenance = "manifest".to_string();
                }
                return Ok(agents);
            }
        }
    }

    if value.get("agents").is_some() {
        let document: ManifestDocument =
            serde_json::from_value(value).map_err(|error| format!("manifest 解析失败：{error}"))?;
        return Ok(document
            .agents
            .into_iter()
            .filter_map(manifest_descriptor)
            .collect());
    }
    let agent: ManifestAgent =
        serde_json::from_value(value).map_err(|error| format!("manifest 解析失败：{error}"))?;
    manifest_descriptor(agent)
        .map(|agent| vec![agent])
        .ok_or_else(|| {
            "manifest 缺少可执行文件名（launch.binNames / binNames / bin / command）".to_string()
        })
}

fn manifest_descriptor(agent: ManifestAgent) -> Option<AgentDescriptor> {
    let mut bin_names = agent
        .launch
        .as_ref()
        .map(|launch| launch.bin_names.clone())
        .unwrap_or_default();
    bin_names.extend(agent.bin_names);
    if let Some(bin) = agent.bin {
        bin_names.push(bin);
    }
    if let Some(command) = agent.command {
        if let Some(name) = binary_bin_name(&command) {
            bin_names.push(name);
        }
    }
    bin_names.retain(|name| !name.is_empty());
    bin_names.sort();
    bin_names.dedup();
    if bin_names.is_empty() {
        return None;
    }
    let launch_args = agent
        .launch
        .as_ref()
        .map(|launch| launch.args.clone())
        .filter(|args| !args.is_empty())
        .unwrap_or(agent.args);
    let protocol = agent.protocol.unwrap_or_else(|| {
        if launch_args.iter().any(|arg| arg == "acp") {
            "acp".to_string()
        } else {
            "unknown".to_string()
        }
    });
    let mut capabilities = agent.capabilities;
    if protocol == "acp" && !capabilities.iter().any(|value| value == "acp") {
        capabilities.push("acp".to_string());
    }
    Some(AgentDescriptor {
        id: agent.id,
        name: agent.name,
        version: agent.version,
        description: agent.description,
        protocol,
        capabilities,
        launch: AgentLaunch {
            bin_names,
            args: launch_args,
            package: agent.launch.and_then(|launch| launch.package),
        },
        version_args: agent.version_args,
        login_args: agent.login_args,
        config_paths: agent.config_paths,
        docs_url: agent.docs_url,
        website: agent.website,
        repository: agent.repository,
        provenance: "manifest".to_string(),
    })
}
