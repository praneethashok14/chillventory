use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{de::DeserializeOwned, Serialize};

use crate::models::Inventory;

#[derive(Debug)]
pub enum StorageError {
    Io(io::Error),
    Corrupt(String),
    Serialize(serde_json::Error),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Io(e) => write!(f, "storage io error: {e}"),
            StorageError::Corrupt(path) => {
                write!(f, "data file is unreadable or malformed: {path}")
            }
            StorageError::Serialize(e) => write!(f, "serialization error: {e}"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<io::Error> for StorageError {
    fn from(value: io::Error) -> Self {
        StorageError::Io(value)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(value: serde_json::Error) -> Self {
        StorageError::Serialize(value)
    }
}

pub struct JsonStore<T: Serialize + DeserializeOwned + Default + Clone> {
    path: PathBuf,
    inner: Mutex<T>,
}

impl<T: Serialize + DeserializeOwned + Default + Clone> JsonStore<T> {
    pub fn load_or_init(path: PathBuf) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let value: T = if path.exists() {
            let bytes = fs::read(&path)?;
            if bytes.is_empty() {
                T::default()
            } else {
                match serde_json::from_slice::<T>(&bytes) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(StorageError::Corrupt(path.display().to_string()));
                    }
                }
            }
        } else {
            let default = T::default();
            write_atomic(&path, &default)?;
            default
        };
        Ok(Self {
            path,
            inner: Mutex::new(value),
        })
    }

    pub fn with_read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let guard = self.inner.lock().expect("store mutex poisoned");
        f(&guard)
    }

    /// Mutate under the lock and persist. If the write fails, the in-memory
    /// state is rolled back to its pre-call snapshot so disk and memory stay
    /// consistent (08-BACKEND-AND-PERSISTENCE.md).
    pub fn mutate<R, F>(&self, f: F) -> Result<R, StorageError>
    where
        F: FnOnce(&mut T) -> Result<R, StorageError>,
    {
        let mut guard = self.inner.lock().expect("store mutex poisoned");
        let snapshot = guard.clone();
        match f(&mut guard) {
            Ok(result) => match write_atomic(&self.path, &*guard) {
                Ok(()) => Ok(result),
                Err(e) => {
                    *guard = snapshot;
                    Err(e)
                }
            },
            Err(e) => {
                *guard = snapshot;
                Err(e)
            }
        }
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub type InventoryStore = JsonStore<Inventory>;

fn write_atomic<T: Serialize>(dest: &Path, value: &T) -> Result<(), StorageError> {
    let serialized = serde_json::to_vec_pretty(value)?;
    let parent = dest
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "destination has no parent"))?;
    fs::create_dir_all(parent)?;
    let file_name = dest
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "data.json".to_string());
    let tmp = parent.join(format!(".{file_name}.tmp"));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&serialized)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn corrupt_file_is_preserved() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("inventory.json");
        fs::write(&path, b"{ not valid json ").unwrap();
        let before = fs::read(&path).unwrap();
        let err = InventoryStore::load_or_init(path.clone()).err().unwrap();
        matches!(err, StorageError::Corrupt(_));
        let after = fs::read(&path).unwrap();
        assert_eq!(before, after, "corrupt file must not be overwritten");
    }

    #[test]
    fn missing_file_is_initialised_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("inventory.json");
        let store = InventoryStore::load_or_init(path.clone()).unwrap();
        assert!(path.exists());
        store.with_read(|inv| {
            assert!(inv.appliances.is_empty());
            assert!(inv.items.is_empty());
        });
    }
}
