use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NetworkState {
    pub is_connected: bool,
    pub is_metered: bool,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<u16>,
}
