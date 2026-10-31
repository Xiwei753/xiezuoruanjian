// =============================================================================
// preferences.rs — Agent CLI 用户选择
// =============================================================================
//
// 只持久化用户选择：选中 Agent、实际执行路径和手工指定的可执行文件。
// 发现到的安装清单不写入配置；旧配置里的第三方 Agent / manifest 会被忽略。
// =============================================================================

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentCliPreferences {
    pub selected_agent_id: String,
    pub selected_launch_path: String,
    pub custom_executables: Vec<String>,
}

impl Default for AgentCliPreferences {
    fn default() -> Self {
        Self {
            selected_agent_id: String::new(),
            selected_launch_path: String::new(),
            custom_executables: Vec::new(),
        }
    }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("sujian").join("agent-cli.json")
}

pub fn load() -> AgentCliPreferences {
    let path = config_path();
    let Ok(text) = fs::read_to_string(path) else {
        return AgentCliPreferences::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save(preferences: &AgentCliPreferences) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建 Agent CLI 配置目录：{error}"))?;
    }
    let text = serde_json::to_string_pretty(preferences)
        .map_err(|error| format!("无法序列化 Agent CLI 配置：{error}"))?;
    let temporary = path.with_extension(format!("json.tmp.{}", std::process::id()));
    fs::write(&temporary, text).map_err(|error| format!("无法写入 Agent CLI 配置：{error}"))?;
    fs::rename(&temporary, &path).map_err(|error| format!("无法保存 Agent CLI 配置：{error}"))
}
