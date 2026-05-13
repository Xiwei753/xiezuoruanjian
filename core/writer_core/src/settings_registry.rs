pub struct SettingsRegistry;

impl SettingsRegistry {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SettingsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_registry_new() {
        let _registry = SettingsRegistry::new();
    }
}
