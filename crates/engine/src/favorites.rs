//! Favorites: connections, tables and queries the user saved.

use quarry_model::Favorite;
use quarry_store::FavoritesStore;

use crate::engine::{Engine, lock};
use crate::{Event, NoticeKind};

impl Engine {
    pub(crate) fn load_favorites(&self) {
        let favorites = match self.favorite_store.as_ref().map(FavoritesStore::load) {
            Some(Ok(list)) => list,
            Some(Err(err)) => {
                self.notice(NoticeKind::Error, format!("Couldn't read favorites: {err}"));
                Vec::new()
            }
            None => Vec::new(),
        };
        *lock(&self.favorites) = favorites.clone();
        self.emit(Event::Favorites { favorites });
    }

    pub(crate) fn persist_favorites(&self) {
        let favorites = lock(&self.favorites).clone();
        if let Some(store) = &self.favorite_store
            && let Err(err) = store.save(&favorites)
        {
            self.notice(NoticeKind::Error, format!("Couldn't save favorites: {err}"));
        }
        self.emit(Event::Favorites { favorites });
    }

    pub(crate) fn delete_favorites(&self, matches: impl Fn(&Favorite) -> bool) {
        let removed = {
            let mut list = lock(&self.favorites);
            let before = list.len();
            list.retain(|f| !matches(f));
            list.len() != before
        };
        if removed {
            self.persist_favorites();
        }
    }

    // ---------- History ----------
}
