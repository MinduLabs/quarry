//! The open-tabs workspace saved between runs.

use quarry_model::Workspace;
use quarry_store::WorkspaceStore;

use crate::engine::Engine;
use crate::{Event, NoticeKind, RequestId};

impl Engine {
    pub(crate) fn load_workspace(&self) {
        let workspace = match self.workspace.as_ref().map(WorkspaceStore::load) {
            Some(Ok(workspace)) => workspace,
            Some(Err(err)) => {
                self.notice(
                    NoticeKind::Warning,
                    format!("Couldn't restore the tabs from the last session: {err}"),
                );
                Workspace::default()
            }
            None => Workspace::default(),
        };
        self.emit(Event::Workspace { workspace });
    }

    pub(crate) fn save_workspace(&self, request: RequestId, workspace: &Workspace) {
        let error = match &self.workspace {
            Some(store) => store.save(workspace).err().map(|err| err.to_string()),
            None => Some("No data directory".to_owned()),
        };
        if let Some(err) = &error {
            tracing::warn!(%err, "workspace save");
        }
        self.emit(Event::WorkspaceSaved { request, error });
    }

    // ---------- Sessions ----------
}
