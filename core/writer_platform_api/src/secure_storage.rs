pub trait SecureStorage: Send + Sync {
    fn get_secret(&self, key: &str) -> Result<Option<Vec<u8>>, String>;
    fn set_secret(&self, key: &str, value: &[u8]) -> Result<(), String>;
    fn delete_secret(&self, key: &str) -> Result<(), String>;
}
