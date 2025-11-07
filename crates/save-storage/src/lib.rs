mod error;
mod layout;

#[cfg(test)]
mod tests;

pub use error::{Result, StorageError};

use layout::StorageLayout;
use std::path::Path;
use tokio::fs;
use tokio::io::{AsyncRead, AsyncWriteExt};
use tracing::debug;

pub struct ObjectStorage {
    layout: StorageLayout,
}

impl ObjectStorage {
    pub async fn new<P: AsRef<Path>>(base_path: P) -> Result<Self> {
        let layout = StorageLayout::new(base_path);

        fs::create_dir_all(layout.objects_dir()).await?;
        fs::create_dir_all(layout.temp_dir()).await?;

        Ok(Self { layout })
    }

    pub async fn put_object<R>(&self, key: &str, mut reader: R) -> Result<()>
    where
        R: AsyncRead + Unpin,
    {
        let id = self.layout.object_id(key)?;
        let temp_path = self.layout.temp_path_from_id(&id);
        let final_path = self.layout.object_path_from_id(&id);

        debug!("Writing object {} to temp path {:?}", key, temp_path);

        let temp_parent = temp_path
            .parent()
            .ok_or_else(|| StorageError::InvalidPath("Temp path has no parent".to_string()))?;
        fs::create_dir_all(temp_parent).await?;

        let mut temp_file = fs::File::create(&temp_path).await?;
        tokio::io::copy(&mut reader, &mut temp_file).await?;
        temp_file.flush().await?;
        drop(temp_file);

        debug!("Moving object {} to final path {:?}", key, final_path);

        let final_parent = final_path
            .parent()
            .ok_or_else(|| StorageError::InvalidPath("Final path has no parent".to_string()))?;
        fs::create_dir_all(final_parent).await?;

        if let Err(e) = fs::rename(&temp_path, &final_path).await {
            let _ = fs::remove_file(&temp_path).await;
            return Err(e.into());
        }

        debug!("Object {} stored successfully", key);

        Ok(())
    }

    pub async fn get_object(&self, key: &str) -> Result<fs::File> {
        let path = self.layout.object_path(key)?;

        debug!("Reading object {} from path {:?}", key, path);

        match fs::File::open(&path).await {
            Ok(file) => Ok(file),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StorageError::NotFound(key.to_string()))
            }
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    pub async fn delete_object(&self, key: &str) -> Result<()> {
        let path = self.layout.object_path(key)?;

        debug!("Deleting object {} at path {:?}", key, path);

        match fs::remove_file(&path).await {
            Ok(_) => {
                debug!("Object {} deleted successfully", key);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StorageError::NotFound(key.to_string()))
            }
            Err(e) => Err(StorageError::Io(e)),
        }
    }
}
