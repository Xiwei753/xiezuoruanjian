use crate::{
    ConfigStore, NetworkState, PlatformInit, SecureStorage, SyncTransport, TransportError,
};
use std::sync::{Arc, OnceLock};

pub type SyncTransportFactory =
    Arc<dyn Fn() -> Result<Box<dyn SyncTransport>, TransportError> + Send + Sync>;

pub struct PlatformServices {
    pub init: PlatformInit,
    pub config_store: Option<Box<dyn ConfigStore>>,
    pub secure_storage: Option<Box<dyn SecureStorage>>,
    pub network_state: Option<NetworkState>,
    pub sync_transport_factory: Option<SyncTransportFactory>,
}

pub trait PlatformServicesResolver: Send + Sync {
    fn resolve(&self, init: &PlatformInit, network_state: &NetworkState) -> PlatformServices;
}

static PLATFORM_SERVICES_RESOLVER: OnceLock<Box<dyn PlatformServicesResolver>> = OnceLock::new();

pub fn register_platform_services_resolver(resolver: Box<dyn PlatformServicesResolver>) {
    PLATFORM_SERVICES_RESOLVER.set(resolver).ok();
}

pub fn get_platform_services_resolver() -> Option<&'static dyn PlatformServicesResolver> {
    PLATFORM_SERVICES_RESOLVER.get().map(|r| r.as_ref())
}
