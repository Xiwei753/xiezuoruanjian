//! Linux Qt PlatformCapabilities 实现

use writer_core::platform_interaction::capabilities::{PlatformCapabilities, PlatformKind};

/// Linux Qt 平台能力适配器
pub struct LinuxQtCapabilitiesAdapter {
    capabilities: PlatformCapabilities,
}

impl LinuxQtCapabilitiesAdapter {
    pub fn new() -> Self {
        Self {
            capabilities: PlatformKind::LinuxQt.default_capabilities(),
        }
    }

    pub fn capabilities(&self) -> &PlatformCapabilities {
        &self.capabilities
    }
}

impl Default for LinuxQtCapabilitiesAdapter {
    fn default() -> Self {
        Self::new()
    }
}
