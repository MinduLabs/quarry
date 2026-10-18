//! Open tabs, saved between launches in `<data dir>/workspace.json`.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use quarry_model::Workspace;
use serde::{Deserialize, Serialize};

use crate::{Result, StoreError, write_atomic};

const WORKSPACE_FILE: &str = "workspace.json";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct WorkspaceFile {
    version: u32,
    #[serde(flatten)]
    workspace: Workspace,
}

#[derive(Debug)]
pub struct WorkspaceStore {
    dir: PathBuf,
    /// Saves arrive as separate engine tasks; they must not interleave on the temp file.
    write: Mutex<()>,
}

impl WorkspaceStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            write: Mutex::new(()),
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.join(WORKSPACE_FILE)
    }

    /// A missing file is an empty workspace. A file that cannot be read is moved aside
    /// (`workspace.json.broken`) so the next save does not destroy it, and the error is returned.
    pub fn load(&self) -> Result<Workspace> {
        let path = self.path();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Workspace::default());
            }
            Err(source) => return Err(StoreError::Io { path, source }),
        };
        let file: WorkspaceFile = match serde_json::from_str(&text) {
            Ok(file) => file,
            Err(source) => {
                let mut broken = path.as_os_str().to_owned();
                broken.push(".broken");
                if let Err(err) = fs::rename(&path, PathBuf::from(broken)) {
                    tracing::warn!(%err, "could not move the broken workspace file aside");
                }
                return Err(StoreError::Json { path, source });
            }
        };
        if file.version > SCHEMA_VERSION {
            return Err(StoreError::TooNew {
                path,
                found: file.version,
            });
        }
        Ok(file.workspace)
    }

    pub fn save(&self, workspace: &Workspace) -> Result<()> {
        let file = WorkspaceFile {
            version: SCHEMA_VERSION,
            workspace: workspace.clone(),
        };
        let path = self.path();
        let bytes = serde_json::to_vec_pretty(&file).map_err(|source| StoreError::Json {
            path: path.clone(),
            source,
        })?;
        let _guard = self.write.lock().unwrap_or_else(|p| p.into_inner());
        write_atomic(&path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use quarry_model::{
        BrowsePage, BrowseQuery, ColumnFilter, ConnId, FilterOp, SavedTab, SortKey, TableRef,
    };

    use super::*;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("quarry-workspace-{}", uuid::Uuid::new_v4()))
    }

    fn sample() -> Workspace {
        Workspace {
            tabs: vec![
                SavedTab {
                    title: "Query 1".into(),
                    conn: ConnId::new(),
                    database: Some("shop".into()),
                    sql: "SELECT 1;\n-- ünïcode".into(),
                    browse: None,
                },
                SavedTab {
                    title: "orders".into(),
                    conn: ConnId::new(),
                    database: Some("shop".into()),
                    sql: String::new(),
                    browse: Some(BrowsePage {
                        table: TableRef {
                            database: Some("shop".into()),
                            schema: None,
                            name: "orders".into(),
                        },
                        offset: 2000,
                        query: BrowseQuery {
                            filters: vec![ColumnFilter {
                                column: "status".into(),
                                op: FilterOp::Eq,
                                value: "paid".into(),
                            }],
                            sort: vec![SortKey {
                                column: "id".into(),
                                descending: true,
                            }],
                        },
                    }),
                },
            ],
            active: Some(1),
        }
    }

    #[test]
    fn missing_file_is_empty_and_save_roundtrips() {
        let dir = temp_dir();
        let store = WorkspaceStore::new(&dir);
        assert_eq!(store.load().unwrap(), Workspace::default());
        let workspace = sample();
        store.save(&workspace).unwrap();
        assert_eq!(store.load().unwrap(), workspace);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn browse_tabs_saved_before_filters_load_unfiltered() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let conn = ConnId::new();
        let old = format!(
            r#"{{"version":1,"tabs":[{{"title":"orders","conn":"{}","database":"shop","sql":"",
            "browse":{{"table":{{"database":"shop","schema":null,"name":"orders"}},"offset":0}}}}],
            "active":0}}"#,
            conn.0
        );
        fs::write(dir.join(WORKSPACE_FILE), old).unwrap();
        let workspace = WorkspaceStore::new(&dir).load().unwrap();
        let browse = workspace.tabs[0].browse.as_ref().unwrap();
        assert_eq!(browse.query, BrowseQuery::default());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn broken_file_is_moved_aside() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(WORKSPACE_FILE), "{ not json").unwrap();
        let store = WorkspaceStore::new(&dir);
        assert!(matches!(store.load(), Err(StoreError::Json { .. })));
        assert!(dir.join("workspace.json.broken").exists());
        assert_eq!(store.load().unwrap(), Workspace::default());
        fs::remove_dir_all(dir).unwrap();
    }
}
