use crate::middleware::RequestTracker;
use save_common::ObjectLockManager;
use save_common::config::SaveConfig;
use save_metadata::MetadataStore;
use save_storage::ObjectStorage;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<ObjectStorage>,
    pub metadata: Arc<MetadataStore>,
    pub config: Arc<SaveConfig>,
    pub lock_manager: Arc<ObjectLockManager>,
    pub request_tracker: Arc<RequestTracker>,
    pub start_time: Instant,
}

impl AppState {
    pub fn new(storage: ObjectStorage, metadata: MetadataStore, config: SaveConfig) -> Self {
        Self {
            storage: Arc::new(storage),
            metadata: Arc::new(metadata),
            config: Arc::new(config),
            lock_manager: Arc::new(ObjectLockManager::new_default()),
            request_tracker: Arc::new(RequestTracker::new()),
            start_time: Instant::now(),
        }
    }
}
