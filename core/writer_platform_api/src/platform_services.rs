use crate::{ConfigStore, NetworkState, PlatformInit, SecureStorage, SyncTransport};
use std::sync::Arc;

pub type SyncTransportFactory = Arc<dyn Fn() -> Box<dyn SyncTransport> + Send + Sync>;

pub struct PlatformServices {
    pub init: PlatformInit,
    pub config_store: Option<Box<dyn ConfigStore>>,
    pub secure_storage: Option<Box<dyn SecureStorage>>,
    pub network_state: Option<NetworkState>,
    pub sync_transport_factory: Option<SyncTransportFactory>,
}
