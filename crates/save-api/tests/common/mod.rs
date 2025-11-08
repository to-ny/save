use save_api::AppState;
use save_common::config::SaveConfig;
use save_metadata::MetadataStore;
use save_storage::ObjectStorage;
use tempfile::TempDir;

pub async fn test_setup() -> (AppState, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let data_path = temp_dir.path().join("data");
    let metadata_path = temp_dir.path().join("metadata");

    let mut config = SaveConfig::default();
    config.storage.data_path = data_path.to_str().unwrap().to_string();
    config.storage.metadata_path = metadata_path.to_str().unwrap().to_string();

    let storage = ObjectStorage::new(&config.storage.data_path)
        .await
        .unwrap();
    let metadata = MetadataStore::new(&config.storage.metadata_path).unwrap();

    metadata.create_bucket("test-bucket").await.unwrap();

    let state = AppState::new(storage, metadata, config);
    (state, temp_dir)
}
