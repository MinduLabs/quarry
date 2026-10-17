//! Pinned tables and saved queries in `<config dir>/favorites.toml`.

use std::fs;
use std::path::PathBuf;

use quarry_model::Favorite;
use serde::{Deserialize, Serialize};

use crate::{Result, StoreError, write_atomic};

const FAVORITES_FILE: &str = "favorites.toml";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct FavoritesFile {
    version: u32,
    #[serde(default, rename = "favorite")]
    favorites: Vec<Favorite>,
}

#[derive(Debug, Clone)]
pub struct FavoritesStore {
    dir: PathBuf,
}

impl FavoritesStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self) -> PathBuf {
        self.dir.join(FAVORITES_FILE)
    }

    /// A missing file means no favorites yet.
    pub fn load(&self) -> Result<Vec<Favorite>> {
        let path = self.path();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(StoreError::Io { path, source }),
        };
        let file: FavoritesFile = toml::from_str(&text).map_err(|source| StoreError::Parse {
            path: path.clone(),
            source: Box::new(source),
        })?;
        if file.version > SCHEMA_VERSION {
            return Err(StoreError::TooNew {
                path,
                found: file.version,
            });
        }
        Ok(file.favorites)
    }

    pub fn save(&self, favorites: &[Favorite]) -> Result<()> {
        let file = FavoritesFile {
            version: SCHEMA_VERSION,
            favorites: favorites.to_vec(),
        };
        let text = toml::to_string_pretty(&file)?;
        write_atomic(&self.path(), text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use quarry_model::{ConnId, FavoriteId, FavoriteItem};

    use super::*;

    #[test]
    fn roundtrip_and_missing_file() {
        let dir = std::env::temp_dir().join(format!("quarry-fav-{}", uuid::Uuid::new_v4()));
        let store = FavoritesStore::new(&dir);
        assert!(store.load().unwrap().is_empty());
        let conn = ConnId::new();
        let list = vec![
            Favorite {
                id: FavoriteId::new(),
                name: "orders".into(),
                conn,
                database: Some("shop".into()),
                item: FavoriteItem::Table {
                    table: "orders".into(),
                },
            },
            Favorite {
                id: FavoriteId::new(),
                name: "Unpaid \"orders\"".into(),
                conn,
                database: None,
                item: FavoriteItem::Query {
                    sql: "SELECT *\nFROM orders\nWHERE status = 'pending';".into(),
                },
            },
        ];
        store.save(&list).unwrap();
        assert_eq!(store.load().unwrap(), list);
        fs::remove_dir_all(dir).unwrap();
    }
}
